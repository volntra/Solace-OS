//! The core's side of `fern_wm_v1` (docs/protocols/wm.md): the window
//! manager is a separate process (fern-wm) that decides where windows go,
//! which one has focus, how they stack; the core reports windows, outputs
//! and input, and carries the decisions out.
//!
//! Only the process the core started may bind the protocol (its client is
//! marked in [`ClientState`]). If it exits or crashes, every window stays
//! where it is, new windows fall back to the core's own placement, and the
//! core starts a new one; on bind that one is told everything, including
//! what its predecessor saved.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    os::unix::net::UnixStream,
    sync::Arc,
    time::{Duration, Instant},
};

use fern_wm_proto::server::fern_wm_v1::{self, FernWmV1};
use smithay::{
    desktop::Window,
    input::{
        keyboard::ModifiersState,
        pointer::{Focus, GrabStartData},
    },
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
            backend::ClientId,
        },
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::{compositor::with_states, shell::xdg::SurfaceCachedState},
};

use crate::{
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab},
    render::Visual,
    spring::{Spring, SpringState},
    state::{ClientState, Comp},
};

/// Starts a window manager and returns the core's end of its connection.
pub type WmLauncher = Box<dyn FnMut() -> std::io::Result<UnixStream>>;

/// Restarts allowed within [`RESTART_WINDOW`] before the core gives up.
const MAX_RESTARTS: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(30);
const RESTART_DELAY: Duration = Duration::from_millis(300);
/// Two presses closer than this (time, distance) are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const DOUBLE_CLICK_SLOP: f64 = 4.0;
const BTN_LEFT: u32 = 0x110;

/// A window's id, kept in its user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinId(pub u32);

/// Where the window manager wants a window drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// Top-left of the frame (titlebar included).
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub alpha: f64,
    pub motion: u32,
}

impl Default for Place {
    fn default() -> Self {
        Place {
            x: 0,
            y: 0,
            scale: 1.0,
            alpha: 1.0,
            motion: 0,
        }
    }
}

/// A window's drawn position, scale and opacity on their way to its
/// [`Place`], each on a spring.
#[derive(Debug, Clone, Copy)]
pub struct Anim {
    pub x: SpringState,
    pub y: SpringState,
    pub scale: SpringState,
    pub alpha: SpringState,
    pub spring: Spring,
    /// Set by the first placement (before it, nothing is drawn anywhere).
    pub started: bool,
}

impl Default for Anim {
    fn default() -> Self {
        Anim {
            x: SpringState::default(),
            y: SpringState::default(),
            scale: SpringState::default(),
            alpha: SpringState::default(),
            spring: Spring::SMOOTH,
            started: false,
        }
    }
}

impl Anim {
    /// At rest exactly at `place`.
    pub fn at(place: &Place) -> Self {
        let rest = |value| SpringState {
            value,
            velocity: 0.0,
        };
        Anim {
            x: rest(f64::from(place.x)),
            y: rest(f64::from(place.y)),
            scale: rest(place.scale),
            alpha: rest(place.alpha),
            spring: preset(place.motion),
            started: true,
        }
    }

    /// Advances towards `place` by `dt` seconds; returns whether it is
    /// still moving (it snaps to rest once closer than anyone can see).
    fn step(&mut self, place: &Place, dt: f64) -> bool {
        let sp = self.spring;
        let mut moving = false;
        for (s, target, eps) in [
            (&mut self.x, f64::from(place.x), 0.05),
            (&mut self.y, f64::from(place.y), 0.05),
            (&mut self.scale, place.scale, 0.0005),
            (&mut self.alpha, place.alpha, 0.002),
        ] {
            if (s.value - target).abs() < eps && s.velocity.abs() < eps * 10.0 {
                *s = SpringState {
                    value: target,
                    velocity: 0.0,
                };
            } else {
                *s = sp.step(*s, target, dt);
                moving = true;
            }
        }
        moving
    }
}

/// The spring for a protocol `motion`.
fn preset(motion: u32) -> Spring {
    match motion {
        1 => Spring::SNAPPY,
        3 => Spring::GENTLE,
        _ => Spring::SMOOTH,
    }
}

/// What the core knows about one window.
// Independent facts about the window, not a state machine in disguise.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct WinRecord {
    pub window: Window,
    /// The window manager's saved data, handed back after a restart.
    pub data: String,
    pub visible: bool,
    pub states: u32,
    pub decorated: bool,
    /// Content size last reported; `None` until the first buffer.
    pub size: Option<(i32, i32)>,
    pub place: Place,
    pub anim: Anim,
    /// Hidden, but still drawn while it fades out; never hit.
    pub leaving: bool,
    /// The client agreed to server-side decoration (xdg-decoration): we
    /// may draw its titlebar.
    pub ssd: bool,
    /// Where a dock shows this window (x, y, w, h; global), for minimize.
    pub icon: Option<(i32, i32, i32, i32)>,
}

/// What the pointer is on (see [`Comp::hit`]).
#[derive(Debug, Clone)]
pub struct Hit {
    pub window: Window,
    pub id: u32,
    pub part: fern_wm_v1::Part,
    /// For `Part::Edge`: which edges (top 1, bottom 2, left 4, right 8).
    pub edges: u32,
    /// Where the window's surface tree is drawn from.
    pub origin: Point<i32, Logical>,
}

/// How far outside a decorated window's frame the resize edges reach, and
/// how long a corner is along each edge.
const EDGE_REACH: f64 = 6.0;
const CORNER: f64 = 14.0;

/// Something the window manager draws above all windows.
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    Rect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        rgba: u32,
    },
    Wallpaper {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
    },
    Mirror {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    Text {
        x: i32,
        y: i32,
        width: i32,
        size: i32,
        rgba: u32,
        align: u32,
        text: String,
    },
}

/// A request waiting for commit.
#[derive(Debug)]
enum Pending {
    Configure(u32, i32, i32, u32),
    Place(u32, Place),
    Decoration(u32, bool),
    Visible(u32, bool),
    Stack(Vec<u32>),
    Focus(u32),
    Close(u32),
    BeginMove(u32),
    BeginResize(u32, u32),
    Save(u32, String),
    SaveWm(String),
    Bind(u32, u32, u32),
    Unbind(u32),
    GrabPointer(bool),
    GrabKeyboard(bool),
    OverlayClear,
    OverlayBeneath(u32),
    Overlay(Overlay),
}

/// What a key press turned into (see [`Comp::key_action`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Binding(u32),
    WmKey {
        keysym: u32,
        pressed: bool,
        mods: u32,
    },
    /// The release of a key whose press went to the window manager.
    Swallow,
    /// One of the shell's global shortcuts.
    Shell(u32),
}

#[derive(Default)]
pub struct WmHost {
    resource: Option<FernWmV1>,
    client: Option<ClientId>,
    next_id: u32,
    pub windows: BTreeMap<u32, WinRecord>,
    pending: Vec<Pending>,
    saved_wm: String,
    bindings: HashMap<u32, (u32, u32)>,
    /// Keys whose press went to the window manager: their release must
    /// not reach a client either.
    swallowed: HashSet<u32>,
    pub grab_pointer: bool,
    pub grab_keyboard: bool,
    pub overlay: Vec<Overlay>,
    pending_overlay: Option<Vec<Overlay>>,
    /// The window the overlay is drawn just below (0: above all).
    pub overlay_beneath: u32,
    pending_beneath: Option<u32>,
    launcher: Option<WmLauncher>,
    restarts: VecDeque<Instant>,
    last_press: Option<(Instant, Point<f64, Logical>, u32)>,
    /// When the animations last advanced.
    last_tick: Option<Instant>,
    /// The window whose traffic lights the pointer is over.
    pub hover: Option<u32>,
}

impl std::fmt::Debug for WmHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WmHost")
            .field("connected", &self.resource.is_some())
            .field("windows", &self.windows.len())
            .finish_non_exhaustive()
    }
}

/// The window manager's modifier bits.
pub fn mod_bits(m: &ModifiersState) -> u32 {
    u32::from(m.shift) | u32::from(m.ctrl) << 1 | u32::from(m.alt) << 2 | u32::from(m.logo) << 3
}

pub fn win_id(window: &Window) -> Option<u32> {
    window.user_data().get::<WinId>().map(|w| w.0)
}

impl WmHost {
    pub fn connected(&self) -> bool {
        self.resource.is_some()
    }

    pub fn record(&self, window: &Window) -> Option<&WinRecord> {
        win_id(window).and_then(|id| self.windows.get(&id))
    }
}

impl Comp {
    /// Creates the `fern_wm_v1` global (visible to the window manager only).
    pub fn init_wm(&mut self) {
        self.display_handle
            .create_global::<Comp, FernWmV1, ()>(1, ());
    }

    /// Disconnects the window manager as if it had crashed (it is then
    /// restarted). For tests, and for restarting it on purpose.
    pub fn kill_wm(&mut self) {
        if let Some(id) = self.wm.client.clone() {
            self.display_handle.backend_handle().kill_client(
                id,
                smithay::reexports::wayland_server::backend::DisconnectReason::ConnectionClosed,
            );
            // The resource's destructor runs at the next client dispatch;
            // act now (it then finds nothing to do).
            if self.wm.resource.is_some() {
                self.wm_gone();
            }
        }
    }

    /// How to start a window manager; starts one now.
    pub fn set_wm_launcher(&mut self, launcher: WmLauncher) {
        self.wm.launcher = Some(launcher);
        self.start_wm();
    }

    /// Starts the window manager (again) and accepts its connection.
    pub fn start_wm(&mut self) {
        let Some(launcher) = self.wm.launcher.as_mut() else {
            return;
        };
        match launcher() {
            Ok(stream) => {
                let data = Arc::new(ClientState {
                    is_wm: true,
                    trusted: true,
                    ..ClientState::default()
                });
                match self.display_handle.insert_client(stream, data) {
                    Ok(client) => self.wm.client = Some(client.id()),
                    Err(e) => tracing::warn!("window manager connection: {e}"),
                }
            }
            Err(e) => tracing::warn!("could not start the window manager: {e}"),
        }
    }

    fn wm_bound(&mut self, resource: &FernWmV1) {
        tracing::info!("window manager connected");
        self.wm.resource = Some(resource.clone());
        // The whole desktop, and what a previous window manager saved.
        for (i, o) in self.space.outputs().enumerate() {
            if let Some(g) = self.space.output_geometry(o) {
                let id = u32::try_from(i).unwrap_or(u32::MAX) + 1;
                resource.output(id, g.loc.x, g.loc.y, g.size.w, g.size.h);
                if let Some(a) = self.shell.work_areas.get(&id) {
                    resource.work_area(id, a.loc.x, a.loc.y, a.size.w, a.size.h);
                }
            }
        }
        for (&id, rec) in &self.wm.windows {
            let (app_id, title) = names(&rec.window);
            resource.window(id, app_id, title, 0);
            send_hints(resource, id, &rec.window);
            resource.window_decoration(id, u32::from(rec.ssd), crate::render::TITLEBAR_HEIGHT);
            if let Some((w, h)) = rec.size {
                resource.window_size(id, w, h);
            }
            if let Some((x, y, w, h)) = rec.icon {
                resource.window_icon(id, x, y, w, h);
            }
            let (x, y) = self.frame_location(rec);
            let (w, h) = rec.size.unwrap_or((0, 0));
            resource.restore(
                id,
                x,
                y,
                w,
                h,
                state_bits(rec.states),
                u32::from(rec.visible),
                rec.data.clone(),
            );
        }
        resource.restore_wm(self.wm.saved_wm.clone());
        resource.done();
    }

    fn wm_gone(&mut self) {
        tracing::warn!("window manager disconnected; windows stay where they are");
        self.wm.resource = None;
        self.wm.client = None;
        self.wm.pending.clear();
        self.wm.bindings.clear();
        self.wm.swallowed.clear();
        self.wm.grab_pointer = false;
        self.wm.grab_keyboard = false;
        self.wm.overlay.clear();
        self.wm.pending_overlay = None;
        self.wm.overlay_beneath = 0;
        self.wm.pending_beneath = None;
        self.request_redraw();
        if self.wm.launcher.is_none() {
            return;
        }
        let now = Instant::now();
        while self
            .wm
            .restarts
            .front()
            .is_some_and(|t| now.duration_since(*t) > RESTART_WINDOW)
        {
            self.wm.restarts.pop_front();
        }
        if self.wm.restarts.len() >= MAX_RESTARTS {
            tracing::error!("the window manager keeps exiting; not restarting it");
            return;
        }
        self.wm.restarts.push_back(now);
        let started =
            self.loop_handle
                .insert_source(Timer::from_duration(RESTART_DELAY), |_, (), data| {
                    data.state.start_wm();
                    TimeoutAction::Drop
                });
        if let Err(e) = started {
            tracing::warn!("scheduling the window manager restart: {}", e.error);
        }
    }

    /// The top-left of a window's frame, from where its content is mapped.
    fn frame_location(&self, rec: &WinRecord) -> (i32, i32) {
        if rec.visible
            && let Some(loc) = self.space.element_location(&rec.window)
        {
            (loc.x, loc.y - self.titlebar_height(rec))
        } else {
            (rec.place.x, rec.place.y)
        }
    }

    /// Height of a window's titlebar: ours when the window manager asked
    /// for it and the client agreed to server-side decoration.
    /// The window manager's overlay, and the window it goes just below
    /// (`None`: above every window).
    pub fn overlay(&self) -> crate::render::OverlayDraw<'_> {
        let beneath = self
            .wm
            .windows
            .get(&self.wm.overlay_beneath)
            .map(|r| r.window.clone());
        crate::render::OverlayDraw {
            items: &self.wm.overlay,
            beneath,
            window: Box::new(|id| self.wm.windows.get(&id).map(|r| r.window.clone())),
        }
    }

    pub fn titlebar_height(&self, rec: &WinRecord) -> i32 {
        if rec.decorated && rec.ssd {
            crate::render::TITLEBAR_HEIGHT
        } else {
            0
        }
    }

    /// The client agreed to (or asked for) server-side decoration.
    pub fn wm_server_side(&mut self, window: &Window) {
        let Some(id) = win_id(window) else {
            return;
        };
        let Some(rec) = self.wm.windows.get_mut(&id) else {
            return;
        };
        if rec.ssd {
            return;
        }
        rec.ssd = true;
        // Already mapped: the window manager hears it now (otherwise with
        // the first window_size).
        if rec.size.is_some()
            && let Some(r) = &self.wm.resource
        {
            r.window_decoration(id, 1, crate::render::TITLEBAR_HEIGHT);
        }
        self.sync_mapping(id);
    }

    // --- Windows: events to the window manager ------------------------------

    /// A new toplevel: give it an id and tell the window manager.
    pub fn wm_new_window(&mut self, window: &Window) {
        self.wm.next_id += 1;
        let id = self.wm.next_id;
        window.user_data().insert_if_missing(|| WinId(id));
        self.wm.windows.insert(
            id,
            WinRecord {
                window: window.clone(),
                data: String::new(),
                visible: false,
                states: 0,
                decorated: false,
                size: None,
                place: Place::default(),
                anim: Anim::default(),
                leaving: false,
                ssd: false,
                icon: None,
            },
        );
        if let Some(r) = &self.wm.resource {
            let (app_id, title) = names(window);
            r.window(id, app_id, title, 0);
        }
    }

    /// A toplevel committed: report its size when it changed. Without a
    /// window manager, a newly mapped window gets the core's placement.
    pub fn wm_window_committed(&mut self, window: &Window) {
        let Some(id) = win_id(window) else {
            return;
        };
        let size = window.geometry().size;
        if size.w <= 0 || size.h <= 0 {
            return;
        }
        let size = (size.w, size.h);
        let Some(rec) = self.wm.windows.get_mut(&id) else {
            return;
        };
        if rec.size == Some(size) {
            return;
        }
        let first = rec.size.is_none();
        rec.size = Some(size);
        let ssd = rec.ssd;
        if first {
            self.toplevel_new(id);
        }
        if let Some(r) = &self.wm.resource {
            if first {
                send_hints(r, id, window);
                r.window_decoration(id, u32::from(ssd), crate::render::TITLEBAR_HEIGHT);
            }
            r.window_size(id, size.0, size.1);
        } else if first {
            self.fallback_place(id);
        }
    }

    /// The core's own placement, while no window manager is running.
    fn fallback_place(&mut self, id: u32) {
        let Some(window) = self.wm.windows.get(&id).map(|r| r.window.clone()) else {
            return;
        };
        let Some(area) = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
        else {
            return;
        };
        let size = window.geometry().size;
        let taken: Vec<(i32, i32)> = self
            .space
            .elements()
            .filter(|w| **w != window)
            .filter_map(|w| self.space.element_location(w))
            .map(|p| (p.x, p.y))
            .collect();
        let (x, y) = crate::placement::place(
            (area.loc.x, area.loc.y, area.size.w, area.size.h),
            (size.w, size.h),
            &taken,
        );
        if let Some(rec) = self.wm.windows.get_mut(&id) {
            rec.visible = true;
            rec.place.x = x;
            rec.place.y = y;
            rec.anim = Anim::at(&rec.place);
        }
        self.space.map_element(window.clone(), (x, y), false);
        self.focus(Some(&window));
    }

    pub fn wm_window_closed(&mut self, window: &Window) {
        let Some(id) = win_id(window) else {
            return;
        };
        self.wm.windows.remove(&id);
        self.toplevel_closed(id);
        self.space.unmap_elem(window);
        if let Some(r) = &self.wm.resource {
            r.window_closed(id);
        } else {
            // Without a window manager the topmost window takes focus.
            let top = self.space.elements().last().cloned();
            self.focus(top.as_ref());
        }
        self.request_redraw();
    }

    pub fn wm_names_changed(&mut self, window: &Window) {
        let Some(id) = win_id(window) else {
            return;
        };
        self.toplevel_changed(id);
        let Some(r) = &self.wm.resource else {
            return;
        };
        let (app_id, title) = names(window);
        r.window_app_id(id, app_id);
        r.window_title(id, title);
    }

    /// A dock says where window `id`'s icon is (0×0: nowhere).
    pub fn wm_icon(&mut self, id: u32, rect: (i32, i32, i32, i32)) {
        let Some(rec) = self.wm.windows.get_mut(&id) else {
            return;
        };
        rec.icon = (rect.2 > 0 && rect.3 > 0).then_some(rect);
        if let Some(r) = &self.wm.resource {
            r.window_icon(id, rect.0, rect.1, rect.2, rect.3);
        }
    }

    /// A client asked for something the window manager decides.
    pub fn wm_client_request(
        &mut self,
        window: &Window,
        kind: fern_wm_v1::RequestKind,
        edges: u32,
    ) -> bool {
        let (Some(id), Some(r)) = (win_id(window), &self.wm.resource) else {
            return false;
        };
        r.window_request(id, kind, fern_wm_v1::Edges::from_bits_truncate(edges));
        true
    }

    /// The shell changed what it keeps of an output (menu bar, dock).
    pub fn wm_work_area(&mut self, id: u32, area: Rectangle<i32, Logical>) {
        if let Some(r) = &self.wm.resource {
            r.work_area(id, area.loc.x, area.loc.y, area.size.w, area.size.h);
        }
    }

    // --- Input: events to the window manager --------------------------------

    /// A pointer button. Returns whether clients should still get it.
    pub fn wm_button(&mut self, pos: Point<f64, Logical>, button: u32, pressed: bool) -> bool {
        let Some(r) = self.wm.resource.clone() else {
            return true;
        };
        let (window, part, edges) = match self.hit(pos) {
            Some(h) => (h.id, h.part, h.edges),
            None if self.wm.grab_pointer => (0, fern_wm_v1::Part::Overlay, 0),
            None => (0, fern_wm_v1::Part::None, 0),
        };
        let mut clicks = 1;
        if pressed {
            let now = Instant::now();
            if let Some((t, p, b)) = self.wm.last_press
                && b == button
                && now.duration_since(t) < DOUBLE_CLICK
                && (p.x - pos.x).hypot(p.y - pos.y) <= DOUBLE_CLICK_SLOP
            {
                clicks = 2;
                self.wm.last_press = None;
            } else {
                self.wm.last_press = Some((now, pos, button));
            }
        }
        let mods = self
            .seat
            .get_keyboard()
            .map_or(0, |k| mod_bits(&k.modifier_state()));
        r.pointer_button(
            window,
            part,
            pos.x,
            pos.y,
            button,
            u32::from(pressed),
            fern_wm_v1::Modifiers::from_bits_truncate(mods),
            clicks,
            fern_wm_v1::Edges::from_bits_truncate(edges),
        );
        // Clients get presses on their own content only.
        !self.wm.grab_pointer && part == fern_wm_v1::Part::Content
    }

    /// Updates which window's traffic lights are hovered.
    pub fn update_hover(&mut self, pos: Point<f64, Logical>) {
        use fern_wm_v1::Part;
        let hover = self.hit(pos).and_then(|h| {
            matches!(h.part, Part::Close | Part::Minimize | Part::Zoom).then_some(h.id)
        });
        if hover != self.wm.hover {
            self.wm.hover = hover;
            self.request_redraw();
        }
    }

    /// Pointer motion. Returns whether clients should still get it.
    pub fn wm_motion(&mut self, pos: Point<f64, Logical>) -> bool {
        if !self.wm.grab_pointer {
            return true;
        }
        if let Some(r) = &self.wm.resource {
            r.pointer_motion(pos.x, pos.y);
        }
        false
    }

    /// Decides what a key is for (called from the keyboard filter).
    pub fn key_action(
        &mut self,
        keycode: u32,
        pressed: bool,
        mods: &ModifiersState,
        keysym: u32,
    ) -> Option<KeyAction> {
        if !pressed && self.wm.swallowed.remove(&keycode) {
            return Some(KeyAction::Swallow);
        }
        let bits = mod_bits(mods);
        if pressed
            && !self.wm.grab_keyboard
            && let Some(id) = self.shell_binding(bits, keysym)
        {
            self.wm.swallowed.insert(keycode);
            return Some(KeyAction::Shell(id));
        }
        self.wm.resource.as_ref()?;
        if self.wm.grab_keyboard {
            if pressed {
                self.wm.swallowed.insert(keycode);
            }
            return Some(KeyAction::WmKey {
                keysym,
                pressed,
                mods: bits,
            });
        }
        if pressed
            && let Some((&id, _)) = self
                .wm
                .bindings
                .iter()
                .find(|(_, (m, k))| *m == bits && *k == keysym)
        {
            self.wm.swallowed.insert(keycode);
            return Some(KeyAction::Binding(id));
        }
        None
    }

    pub fn wm_key_event(&mut self, action: KeyAction) {
        if let KeyAction::Shell(id) = action {
            self.shell_key_binding(id);
            return;
        }
        let Some(r) = &self.wm.resource else {
            return;
        };
        match action {
            KeyAction::Binding(id) => r.binding(id),
            KeyAction::WmKey {
                keysym,
                pressed,
                mods,
            } => r.key(
                keysym,
                u32::from(pressed),
                fern_wm_v1::Modifiers::from_bits_truncate(mods),
            ),
            KeyAction::Swallow | KeyAction::Shell(_) => {}
        }
    }

    /// The window is being moved; the pointer is at `p`. (Called from
    /// inside the move grab, where the pointer is locked: `p` must come
    /// from the grab, not from the pointer.)
    pub fn wm_move_update(&mut self, window: &Window, p: Point<f64, Logical>, end: bool) {
        let (Some(id), Some(r)) = (win_id(window), self.wm.resource.clone()) else {
            return;
        };
        let Some(rec) = self.wm.windows.get(&id) else {
            return;
        };
        let (x, y) = self.frame_location(rec);
        // The window follows the pointer exactly; its spring rests there.
        if let Some(rec) = self.wm.windows.get_mut(&id) {
            rec.place.x = x;
            rec.place.y = y;
            rec.anim = Anim::at(&rec.place);
        }
        if end {
            r.move_end(id, x, y, p.x, p.y);
        } else {
            r.move_update(id, x, y, p.x, p.y);
        }
    }

    pub fn wm_resize_end(&mut self, window: &Window) {
        let (Some(id), Some(r)) = (win_id(window), self.wm.resource.clone()) else {
            return;
        };
        let Some(rec) = self.wm.windows.get(&id) else {
            return;
        };
        let (x, y) = self.frame_location(rec);
        let size = window.geometry().size;
        if let Some(rec) = self.wm.windows.get_mut(&id) {
            rec.place.x = x;
            rec.place.y = y;
            rec.anim = Anim::at(&rec.place);
        }
        r.resize_end(id, x, y, size.w, size.h);
    }

    // --- Requests from the window manager -----------------------------------

    // One arm per protocol request: a table, long by nature.
    #[allow(clippy::too_many_lines)]
    fn wm_request(&mut self, request: fern_wm_v1::Request) {
        use fern_wm_v1::Request as R;
        let p = match request {
            R::Configure {
                id,
                width,
                height,
                states,
            } => Pending::Configure(id, width, height, enum_bits(states)),
            R::Place {
                id,
                x,
                y,
                scale,
                alpha,
                motion,
            } => Pending::Place(
                id,
                Place {
                    x,
                    y,
                    scale: scale.clamp(0.01, 4.0),
                    alpha: alpha.clamp(0.0, 1.0),
                    motion: enum_u32(motion),
                },
            ),
            R::Decoration { id, mode } => Pending::Decoration(
                id,
                enum_u32(mode) == fern_wm_v1::Decoration::Titlebar as u32,
            ),
            R::Visible { id, visible } => Pending::Visible(id, visible != 0),
            R::Stack { ids } => Pending::Stack(
                ids.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| u32::from_ne_bytes(*c))
                    .collect(),
            ),
            R::Focus { id } => Pending::Focus(id),
            R::Close { id } => Pending::Close(id),
            R::BeginMove { id } => Pending::BeginMove(id),
            R::BeginResize { id, edges } => Pending::BeginResize(id, enum_bits(edges)),
            R::Save { id, data } => Pending::Save(id, data),
            R::SaveWm { data } => Pending::SaveWm(data),
            R::Bind {
                id,
                modifiers,
                keysym,
            } => Pending::Bind(id, enum_bits(modifiers), keysym),
            R::Unbind { id } => Pending::Unbind(id),
            R::GrabPointer { grab } => Pending::GrabPointer(grab != 0),
            R::GrabKeyboard { grab } => Pending::GrabKeyboard(grab != 0),
            R::OverlayClear => Pending::OverlayClear,
            R::OverlayBeneath { id } => Pending::OverlayBeneath(id),
            R::OverlayRect {
                x,
                y,
                width,
                height,
                radius,
                rgba,
            } => Pending::Overlay(Overlay::Rect {
                x,
                y,
                w: width,
                h: height,
                radius,
                rgba,
            }),
            R::OverlayWallpaper {
                x,
                y,
                width,
                height,
                radius,
            } => Pending::Overlay(Overlay::Wallpaper {
                x,
                y,
                w: width,
                h: height,
                radius,
            }),
            R::OverlayMirror {
                id,
                x,
                y,
                width,
                height,
            } => Pending::Overlay(Overlay::Mirror {
                id,
                x,
                y,
                w: width,
                h: height,
            }),
            R::OverlayText {
                x,
                y,
                width,
                size,
                rgba,
                align,
                text,
            } => Pending::Overlay(Overlay::Text {
                x,
                y,
                width,
                size,
                rgba,
                align: enum_u32(align),
                text,
            }),
            R::Commit => {
                self.wm_commit();
                return;
            }
            _ => return,
        };
        self.wm.pending.push(p);
    }

    /// Applies everything since the last commit, in order.
    fn wm_commit(&mut self) {
        let pending = std::mem::take(&mut self.wm.pending);
        for p in pending {
            self.wm_apply(p);
        }
        if let Some(b) = self.wm.pending_beneath.take() {
            self.wm.overlay_beneath = b;
        }
        if let Some(o) = self.wm.pending_overlay.take() {
            self.wm.overlay = o;
        }
        self.request_redraw();
    }

    // One arm per request: a table, long by nature.
    #[allow(clippy::too_many_lines)]
    fn wm_apply(&mut self, p: Pending) {
        match p {
            Pending::Configure(id, w, h, states) => self.wm_configure(id, w, h, states),
            Pending::Place(id, place) => {
                if let Some(rec) = self.wm.windows.get_mut(&id) {
                    rec.place = place;
                    if place.motion == 0 || !rec.anim.started {
                        rec.anim = Anim::at(&place);
                    } else {
                        rec.anim.spring = preset(place.motion);
                    }
                }
                self.sync_mapping(id);
            }
            Pending::Decoration(id, on) => {
                if let Some(rec) = self.wm.windows.get_mut(&id) {
                    rec.decorated = on;
                }
                self.sync_mapping(id);
            }
            Pending::Visible(id, on) => {
                if let Some(rec) = self.wm.windows.get_mut(&id) {
                    rec.visible = on;
                }
                self.sync_mapping(id);
            }
            Pending::Stack(ids) => {
                for id in ids {
                    if let Some(rec) = self.wm.windows.get(&id)
                        && rec.visible
                    {
                        let w = rec.window.clone();
                        self.space.raise_element(&w, false);
                    }
                }
            }
            Pending::Focus(id) => {
                let surface = self
                    .wm
                    .windows
                    .get(&id)
                    .and_then(|r| r.window.toplevel())
                    .map(|t| t.wl_surface().clone());
                if let Some(keyboard) = self.seat.get_keyboard() {
                    keyboard.set_focus(self, surface, SERIAL_COUNTER.next_serial());
                }
            }
            Pending::Close(id) => {
                if let Some(t) = self.wm.windows.get(&id).and_then(|r| r.window.toplevel()) {
                    t.send_close();
                }
            }
            Pending::BeginMove(id) => self.wm_begin_move(id),
            Pending::BeginResize(id, edges) => self.wm_begin_resize(id, edges),
            Pending::Save(id, data) => {
                if let Some(rec) = self.wm.windows.get_mut(&id) {
                    rec.data = data;
                }
            }
            Pending::SaveWm(data) => self.wm.saved_wm = data,
            Pending::Bind(id, mods, keysym) => {
                self.wm.bindings.insert(id, (mods, keysym));
            }
            Pending::Unbind(id) => {
                self.wm.bindings.remove(&id);
            }
            Pending::GrabPointer(on) => {
                self.wm.grab_pointer = on;
                if on && let Some(pointer) = self.seat.get_pointer() {
                    // Clients lose the pointer while the window manager has it.
                    let location = pointer.current_location();
                    pointer.motion(
                        self,
                        None,
                        &smithay::input::pointer::MotionEvent {
                            location,
                            serial: SERIAL_COUNTER.next_serial(),
                            time: 0,
                        },
                    );
                    pointer.frame(self);
                }
            }
            Pending::GrabKeyboard(on) => {
                // Taking the keyboard, the window manager hears which
                // modifiers are already held (a key event with keysym 0):
                // the app switcher must know if Super is still down.
                if on
                    && !self.wm.grab_keyboard
                    && let (Some(r), Some(k)) = (&self.wm.resource, self.seat.get_keyboard())
                {
                    r.key(
                        0,
                        0,
                        fern_wm_v1::Modifiers::from_bits_truncate(mod_bits(&k.modifier_state())),
                    );
                }
                self.wm.grab_keyboard = on;
            }
            Pending::OverlayClear => {
                self.wm.pending_overlay = Some(Vec::new());
                self.wm.pending_beneath = Some(0);
            }
            Pending::OverlayBeneath(id) => self.wm.pending_beneath = Some(id),
            Pending::Overlay(o) => self
                .wm
                .pending_overlay
                .get_or_insert_with(|| self.wm.overlay.clone())
                .push(o),
        }
    }

    fn wm_configure(&mut self, id: u32, w: i32, h: i32, states: u32) {
        let Some(rec) = self.wm.windows.get_mut(&id) else {
            return;
        };
        let changed = rec.states != states;
        rec.states = states;
        let Some(toplevel) = rec.window.toplevel().cloned() else {
            return;
        };
        if changed {
            self.toplevel_changed(id);
        }
        toplevel.with_pending_state(|s| {
            use xdg_toplevel::State as S;
            s.size = (w > 0 && h > 0).then(|| (w, h).into());
            for (bit, st) in [
                (1, S::Activated),
                (2, S::Maximized),
                (4, S::Fullscreen),
                (8, S::TiledLeft),
                (16, S::TiledRight),
                (32, S::TiledTop),
                (64, S::TiledBottom),
                (128, S::Resizing),
            ] {
                if states & bit != 0 {
                    s.states.set(st);
                } else {
                    s.states.unset(st);
                }
            }
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    /// Puts a window in the space where its record says, or takes it out
    /// (once its last placement comes to rest, when it is moving).
    fn sync_mapping(&mut self, id: u32) {
        let Some(rec) = self.wm.windows.get(&id) else {
            return;
        };
        let window = rec.window.clone();
        let mapped = self.space.elements().any(|w| *w == window);
        // Hidden on its way somewhere (a fade, a slide to another Space):
        // drawn until the spring rests, then unmapped.
        let fades_out = rec.anim.started && rec.place.motion != 0;
        if rec.visible {
            let tb = self.titlebar_height(rec);
            let loc = (rec.place.x, rec.place.y + tb);
            if let Some(rec) = self.wm.windows.get_mut(&id) {
                rec.leaving = false;
            }
            // Mapping raises; moving a window must not restack it (the
            // window manager decides stacking): the ones above go back on
            // top.
            let above: Vec<Window> = if mapped {
                self.space
                    .elements()
                    .skip_while(|w| **w != window)
                    .skip(1)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            self.space.map_element(window, loc, false);
            for w in &above {
                self.space.raise_element(w, false);
            }
        } else if mapped && fades_out {
            if let Some(rec) = self.wm.windows.get_mut(&id) {
                rec.leaving = true;
            }
        } else {
            self.space.unmap_elem(&window);
        }
        self.request_redraw();
    }

    /// Moves every animation on by the time since the last call; returns
    /// whether any is still moving (render again soon). Windows that have
    /// finished fading out leave the space.
    pub fn advance_animations(&mut self) -> bool {
        let now = Instant::now();
        // Real time, so a slow renderer (software, in a VM) shows short
        // animations, not slow motion; the springs are exact for any step.
        // An idle display has no last tick, so idleness is never a jump;
        // a long hitch moves at most one second on.
        let dt = self
            .wm
            .last_tick
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64().min(1.0));
        self.wm.last_tick = Some(now);
        let mut moving = false;
        let mut gone = Vec::new();
        for rec in self.wm.windows.values_mut() {
            if !rec.anim.started {
                continue;
            }
            let m = rec.anim.step(&rec.place, dt);
            moving |= m;
            if rec.leaving && !m {
                gone.push(rec.window.clone());
                rec.leaving = false;
            }
        }
        for w in gone {
            self.space.unmap_elem(&w);
        }
        if !moving {
            self.wm.last_tick = None;
        }
        moving
    }

    /// How a window is drawn now (see [`Visual`]).
    pub fn visual(&self, window: &Window) -> Visual {
        let Some(rec) = self.wm.record(window) else {
            return Visual::IDENTITY;
        };
        let deco = Visual {
            titlebar: self.titlebar_height(rec),
            hover: self.wm.hover.is_some_and(|h| Some(h) == win_id(window)),
            // The protocol's `fullscreen` state.
            square: rec.states & 4 != 0,
            dark: self.dark,
            ..Visual::IDENTITY
        };
        if !rec.anim.started {
            return deco;
        }
        // The spring's remaining way to its target: a window moved
        // directly (a drag) is drawn where it is.
        #[allow(clippy::cast_possible_truncation)]
        Visual {
            dx: rec.anim.x.value - f64::from(rec.place.x),
            dy: rec.anim.y.value - f64::from(rec.place.y),
            scale: rec.anim.scale.value.max(0.0),
            alpha: rec.anim.alpha.value.clamp(0.0, 1.0) as f32,
            ..deco
        }
    }

    /// The window whose content takes input at `pos`, and where its
    /// surface tree is drawn from (`None` over a titlebar or edge).
    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<(Window, Point<i32, Logical>)> {
        self.hit(pos)
            .filter(|h| h.part == fern_wm_v1::Part::Content)
            .map(|h| (h.window, h.origin))
    }

    /// What is under `pos`: the topmost window's content, titlebar, one of
    /// its traffic lights, or a resize edge (windows fading out are not
    /// hit).
    pub fn hit(&self, pos: Point<f64, Logical>) -> Option<Hit> {
        use crate::render::{BUTTON_DIAMETER, BUTTON_FIRST_X, BUTTON_STEP};
        use fern_wm_v1::Part;
        use smithay::desktop::space::SpaceElement;
        // The shell's menu bar, dock and panels are over every window.
        if self.layer_at(pos, &crate::layers::UPPER).is_some() {
            return None;
        }
        for w in self.space.elements().rev() {
            let Some(rec) = self.wm.record(w) else {
                continue;
            };
            if rec.leaving {
                continue;
            }
            let Some(loc) = self.space.element_location(w) else {
                continue;
            };
            let id = win_id(w).unwrap_or(0);
            let origin = loc - w.geometry().loc;
            let hit = |part, edges| Hit {
                window: w.clone(),
                id,
                part,
                edges,
                origin,
            };
            if w.is_in_input_region(&(pos - origin.to_f64())) {
                return Some(hit(Part::Content, 0));
            }
            let tb = f64::from(self.titlebar_height(rec));
            if tb <= 0.0 {
                continue;
            }
            let size = w.geometry().size;
            let (fx, fy) = (f64::from(loc.x), f64::from(loc.y) - tb);
            let (fw, fh) = (f64::from(size.w), f64::from(size.h) + tb);
            let inside = |x: f64, y: f64| x >= fx && x < fx + fw && y >= fy && y < fy + fh;
            if inside(pos.x, pos.y) && pos.y < fy + tb {
                let cy = fy + tb / 2.0;
                for (i, part) in [Part::Close, Part::Minimize, Part::Zoom]
                    .into_iter()
                    .enumerate()
                {
                    #[allow(clippy::cast_precision_loss)]
                    let cx = fx + BUTTON_FIRST_X + BUTTON_STEP * i as f64;
                    if (pos.x - cx).hypot(pos.y - cy) <= BUTTON_DIAMETER / 2.0 + 2.0 {
                        return Some(hit(part, 0));
                    }
                }
                return Some(hit(Part::Titlebar, 0));
            }
            let near = pos.x >= fx - EDGE_REACH
                && pos.x < fx + fw + EDGE_REACH
                && pos.y >= fy - EDGE_REACH
                && pos.y < fy + fh + EDGE_REACH;
            if near && !inside(pos.x, pos.y) {
                // Which sides; near a corner, both.
                let mut e = 0;
                if pos.y < fy + CORNER {
                    e |= 1;
                }
                if pos.y >= fy + fh - CORNER {
                    e |= 2;
                }
                if pos.x < fx + CORNER {
                    e |= 4;
                }
                if pos.x >= fx + fw - CORNER {
                    e |= 8;
                }
                return Some(hit(Part::Edge, e));
            }
        }
        None
    }

    fn wm_begin_move(&mut self, id: u32) {
        let Some(window) = self.wm.windows.get(&id).map(|r| r.window.clone()) else {
            return;
        };
        let (Some(pointer), Some(loc)) = (
            self.seat.get_pointer(),
            self.space.element_location(&window),
        ) else {
            return;
        };
        let start_data = GrabStartData {
            focus: None,
            button: BTN_LEFT,
            location: pointer.current_location(),
        };
        let grab = MoveSurfaceGrab {
            start_data,
            initial_width: window.geometry().size.w,
            window,
            initial_window_location: loc,
        };
        pointer.set_grab(self, grab, SERIAL_COUNTER.next_serial(), Focus::Clear);
    }

    fn wm_begin_resize(&mut self, id: u32, edges: u32) {
        let Some(window) = self.wm.windows.get(&id).map(|r| r.window.clone()) else {
            return;
        };
        let (Some(pointer), Some(loc), Some(toplevel)) = (
            self.seat.get_pointer(),
            self.space.element_location(&window),
            window.toplevel().cloned(),
        ) else {
            return;
        };
        let Some(edges) = crate::grabs::ResizeEdge::from_bits(edges) else {
            return;
        };
        let start_data = GrabStartData {
            focus: None,
            button: BTN_LEFT,
            location: pointer.current_location(),
        };
        toplevel.with_pending_state(|s| s.states.set(xdg_toplevel::State::Resizing));
        toplevel.send_pending_configure();
        let rect = smithay::utils::Rectangle::new(loc, window.geometry().size);
        let grab = ResizeSurfaceGrab::start(start_data, window, edges, rect);
        pointer.set_grab(self, grab, SERIAL_COUNTER.next_serial(), Focus::Clear);
    }
}

pub(crate) fn names(window: &Window) -> (String, String) {
    window.toplevel().map_or_else(Default::default, |t| {
        with_states(t.wl_surface(), |states| {
            states
                .data_map
                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok())
                .map(|d| {
                    (
                        d.app_id.clone().unwrap_or_default(),
                        d.title.clone().unwrap_or_default(),
                    )
                })
                .unwrap_or_default()
        })
    })
}

fn send_hints(r: &FernWmV1, id: u32, window: &Window) {
    let Some(t) = window.toplevel() else {
        return;
    };
    let (min, max) = with_states(t.wl_surface(), |states| {
        let mut cached = states.cached_state.get::<SurfaceCachedState>();
        let c = cached.current();
        (c.min_size, c.max_size)
    });
    r.window_hints(id, min.w, min.h, max.w, max.h);
}

fn state_bits(bits: u32) -> fern_wm_v1::State {
    fern_wm_v1::State::from_bits_truncate(bits)
}

fn enum_u32<T: Into<u32> + Copy>(e: WEnum<T>) -> u32 {
    match e {
        WEnum::Value(v) => v.into(),
        WEnum::Unknown(u) => u,
    }
}

fn enum_bits<T: bitflags::Flags<Bits = u32>>(e: WEnum<T>) -> u32 {
    match e {
        WEnum::Value(v) => v.bits(),
        WEnum::Unknown(u) => u,
    }
}

impl GlobalDispatch<FernWmV1, ()> for Comp {
    fn bind(
        state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<FernWmV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let r = data_init.init(resource, ());
        if state.wm.resource.as_ref().is_some_and(Resource::is_alive) {
            // One window manager at a time.
            r.post_error(0u32, "a window manager is already bound");
            return;
        }
        state.wm_bound(&r);
    }

    fn can_view(client: Client, _global_data: &()) -> bool {
        client.get_data::<ClientState>().is_some_and(|d| d.is_wm)
    }
}

impl Dispatch<FernWmV1, ()> for Comp {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &FernWmV1,
        request: fern_wm_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        state.wm_request(request);
    }

    fn destroyed(state: &mut Self, _client: ClientId, resource: &FernWmV1, _data: &()) {
        if state.wm.resource.as_ref() == Some(resource) {
            state.wm_gone();
        }
    }
}

#[cfg(test)]
// Exact values are the point: a spring at rest is snapped to its target.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn run(anim: &mut Anim, place: &Place, seconds: f64) -> Vec<f64> {
        let mut xs = Vec::new();
        let mut t = 0.0;
        while t < seconds {
            t += 1.0 / 60.0;
            anim.step(place, 1.0 / 60.0);
            xs.push(anim.x.value);
        }
        xs
    }

    #[test]
    fn a_placement_glides_there_without_overshoot_and_comes_to_rest() {
        let from = Place {
            x: 0,
            y: 0,
            ..Place::default()
        };
        let to = Place {
            x: 400,
            y: 100,
            motion: 2,
            ..Place::default()
        };
        let mut anim = Anim::at(&from);
        anim.spring = preset(to.motion);
        let xs = run(&mut anim, &to, 2.0);
        assert!(xs[2] > 0.0 && xs[2] < 400.0, "moves gradually: {}", xs[2]);
        assert!(
            xs.windows(2).all(|w| w[1] >= w[0]),
            "monotonic (critically damped)"
        );
        assert!(xs.iter().all(|&x| x <= 400.0), "no overshoot");
        assert_eq!(anim.x.value, 400.0, "snapped to rest");
        assert_eq!(anim.y.value, 100.0);
        assert!(!anim.step(&to, 1.0 / 60.0), "at rest it reports no motion");
    }

    #[test]
    fn scale_and_alpha_ride_the_same_spring() {
        let from = Place {
            scale: 0.96,
            alpha: 0.0,
            ..Place::default()
        };
        let to = Place {
            motion: 1,
            ..Place::default()
        };
        let mut anim = Anim::at(&from);
        anim.spring = preset(to.motion);
        anim.step(&to, 1.0 / 60.0);
        assert!(anim.alpha.value > 0.0 && anim.alpha.value < 1.0);
        assert!(anim.scale.value > 0.96 && anim.scale.value < 1.0);
        run(&mut anim, &to, 1.0);
        assert_eq!((anim.scale.value, anim.alpha.value), (1.0, 1.0));
    }

    #[test]
    fn presets_follow_the_hig() {
        assert_eq!(preset(1), Spring::SNAPPY);
        assert_eq!(preset(2), Spring::SMOOTH);
        assert_eq!(preset(3), Spring::GENTLE);
    }
}
