//! Window policy, free of Wayland: events from the core go in, commands
//! for it come out. Everything fern-wm decides is here, so it is tested
//! without a compositor (`cargo test -p fern-wm`).

use std::collections::BTreeMap;

mod overview;
mod spaces;
mod switcher;
mod tile;
pub use overview::{Overview, grid};
pub use spaces::{SpaceInfo, Windowed};
pub use switcher::{Switcher, app_name};
pub use tile::{Tile, TileKey};

/// `xdg_toplevel` states (the protocol's `state` bits).
pub mod state {
    pub const ACTIVATED: u32 = 1;
    pub const MAXIMIZED: u32 = 2;
    pub const FULLSCREEN: u32 = 4;
    pub const TILED_LEFT: u32 = 8;
    pub const TILED_RIGHT: u32 = 16;
    pub const TILED_TOP: u32 = 32;
    pub const TILED_BOTTOM: u32 = 64;
    /// Ours, not xdg's: the dock and switcher see the window minimized.
    pub const MINIMIZED: u32 = 256;
}

/// The protocol's `part` values.
pub mod part {
    pub const NONE: u32 = 0;
    pub const CONTENT: u32 = 1;
    pub const TITLEBAR: u32 = 2;
    pub const CLOSE: u32 = 3;
    pub const MINIMIZE: u32 = 4;
    pub const ZOOM: u32 = 5;
    pub const EDGE: u32 = 6;
    pub const OVERLAY: u32 = 7;
}

/// The protocol's `request_kind` values.
pub mod request {
    pub const MOVE: u32 = 1;
    pub const RESIZE: u32 = 2;
    pub const MAXIMIZE: u32 = 3;
    pub const UNMAXIMIZE: u32 = 4;
    pub const FULLSCREEN: u32 = 5;
    pub const UNFULLSCREEN: u32 = 6;
    pub const MINIMIZE: u32 = 7;
    pub const ACTIVATE: u32 = 8;
}

/// Spring presets (the protocol's `motion`).
pub mod motion {
    pub const INSTANT: u32 = 0;
    pub const SNAPPY: u32 = 1;
    pub const SMOOTH: u32 = 2;
    pub const GENTLE: u32 = 3;
}

/// Modifier bits.
pub mod mods {
    pub const SHIFT: u32 = 1;
    pub const CTRL: u32 = 2;
    pub const ALT: u32 = 4;
    pub const SUPER: u32 = 8;
}

/// What the core tells the window manager.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Output {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    OutputRemoved(u32),
    /// The part of an output left for windows (without the menu bar and
    /// the dock).
    WorkArea {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    Window {
        id: u32,
        app_id: String,
        title: String,
        parent: u32,
    },
    Title(u32, String),
    AppId(u32, String),
    Hints {
        id: u32,
        min: (i32, i32),
        max: (i32, i32),
    },
    Size {
        id: u32,
        w: i32,
        h: i32,
    },
    Closed(u32),
    Request {
        id: u32,
        kind: u32,
        edges: u32,
    },
    Restore {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        states: u32,
        visible: bool,
        data: String,
    },
    RestoreWm(String),
    Done,
    /// Where a dock shows the window (minimize goes there).
    Icon {
        id: u32,
        rect: Option<Rect>,
    },
    /// Whether we draw the titlebar (server side) and how tall it is.
    Decoration {
        id: u32,
        server_side: bool,
        titlebar: i32,
    },
    Button {
        window: u32,
        part: u32,
        x: f64,
        y: f64,
        button: u32,
        pressed: bool,
        mods: u32,
        clicks: u32,
        edges: u32,
    },
    Motion {
        x: f64,
        y: f64,
    },
    Key {
        keysym: u32,
        pressed: bool,
        mods: u32,
    },
    Binding(u32),
    MoveUpdate {
        id: u32,
        x: i32,
        y: i32,
        px: f64,
        py: f64,
    },
    MoveEnd {
        id: u32,
        x: i32,
        y: i32,
        px: f64,
        py: f64,
    },
    ResizeEnd {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
}

/// What the window manager asks of the core.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Configure {
        id: u32,
        w: i32,
        h: i32,
        states: u32,
    },
    Place {
        id: u32,
        x: i32,
        y: i32,
        scale: f64,
        alpha: f64,
        motion: u32,
    },
    Decoration {
        id: u32,
        titlebar: bool,
    },
    Visible {
        id: u32,
        visible: bool,
    },
    Stack(Vec<u32>),
    Focus(u32),
    Close(u32),
    BeginMove(u32),
    BeginResize(u32, u32),
    Save(u32, String),
    SaveWm(String),
    Bind {
        id: u32,
        mods: u32,
        keysym: u32,
    },
    GrabPointer(bool),
    GrabKeyboard(bool),
    OverlayClear,
    OverlayBeneath(u32),
    OverlayRect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        rgba: u32,
    },
    OverlayWallpaper {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
    },
    OverlayMirror {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    OverlayText {
        x: i32,
        y: i32,
        width: i32,
        size: i32,
        rgba: u32,
        align: u32,
        text: String,
    },
}

/// The window manager's key bindings: (id, modifiers, keysym).
pub mod keys {
    use super::mods::{CTRL, SHIFT, SUPER};
    pub const TILE_LEFT: u32 = 1;
    pub const TILE_RIGHT: u32 = 2;
    pub const TILE_UP: u32 = 3;
    pub const TILE_DOWN: u32 = 4;
    pub const SPACE_LEFT: u32 = 5;
    pub const SPACE_RIGHT: u32 = 6;
    pub const CARRY_LEFT: u32 = 7;
    pub const CARRY_RIGHT: u32 = 8;
    pub const FULLSCREEN: u32 = 9;
    pub const MINIMIZE: u32 = 10;
    pub const OVERVIEW: u32 = 11;
    pub const SWITCH_APPS: u32 = 12;
    pub const SWITCH_APPS_BACK: u32 = 13;
    pub const CYCLE_WINDOWS: u32 = 14;
    const TAB: u32 = 0xff09;
    const GRAVE: u32 = 0x60;
    const LEFT: u32 = 0xff51;
    const UP: u32 = 0xff52;
    const RIGHT: u32 = 0xff53;
    const DOWN: u32 = 0xff54;
    pub const BINDINGS: &[(u32, u32, u32)] = &[
        (TILE_LEFT, SUPER, LEFT),
        (TILE_RIGHT, SUPER, RIGHT),
        (TILE_UP, SUPER, UP),
        (TILE_DOWN, SUPER, DOWN),
        (SPACE_LEFT, CTRL | SUPER, LEFT),
        (SPACE_RIGHT, CTRL | SUPER, RIGHT),
        (CARRY_LEFT, CTRL | SUPER | SHIFT, LEFT),
        (CARRY_RIGHT, CTRL | SUPER | SHIFT, RIGHT),
        (FULLSCREEN, CTRL | SUPER, 0x66),
        (MINIMIZE, SUPER, 0x6d),
        (OVERVIEW, CTRL | SUPER, UP),
        (SWITCH_APPS, SUPER, TAB),
        (SWITCH_APPS_BACK, SUPER | SHIFT, TAB),
        (CYCLE_WINDOWS, SUPER, GRAVE),
    ];
}

/// A window opens from this far below its place, at this scale.
const OPEN_DROP: i32 = 14;
const OPEN_SCALE: f64 = 0.96;

/// A rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    /// Area shared with `o`.
    pub fn overlap(&self, o: &Rect) -> i64 {
        let w = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let h = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        if w <= 0 || h <= 0 {
            0
        } else {
            i64::from(w) * i64::from(h)
        }
    }
}

/// One window, as the window manager sees it.
#[derive(Debug, Clone, Default)]
pub struct Win {
    pub app_id: String,
    pub title: String,
    pub parent: u32,
    /// Content size the client last committed; `None` until mapped.
    pub size: Option<(i32, i32)>,
    /// Content size we last configured (0: the client chooses).
    pub cfg: (i32, i32),
    /// Height of our titlebar above the content (0: none).
    pub tb: i32,
    /// Zoomed: the frame it had before, to return to.
    pub unzoomed: Option<Rect>,
    /// Minimized (hidden until restored).
    pub minimized: bool,
    /// Tiled into a half or quarter, and the free frame (content size) it
    /// goes back to.
    pub tile: Option<Tile>,
    pub untiled: Option<Rect>,
    /// The Space it is on.
    pub space: u32,
    /// Fullscreen (no titlebar, its own Space): what it goes back to.
    pub windowed: Option<Windowed>,
    /// What the core holds for us (`save`), to notice changes.
    saved: String,
    /// Its icon in the dock, if a dock shows it.
    pub icon: Option<Rect>,
    pub x: i32,
    pub y: i32,
    pub visible: bool,
    pub states: u32,
    pub min: (i32, i32),
    pub max: (i32, i32),
}

impl Win {
    pub fn mapped(&self) -> bool {
        self.size.is_some()
    }

    /// The window manager's memory of this window, as `save` data:
    /// `key=value` pairs separated by `;`.
    fn dump(&self) -> String {
        let rect = |r: Rect| format!("{},{},{},{}", r.x, r.y, r.w, r.h);
        let mut parts = vec![format!("space={}", self.space)];
        // Hidden, it is drawn where it went (off screen, into the dock):
        // where it belongs is ours to remember.
        if !self.visible {
            parts.push(format!("at={},{}", self.x, self.y));
        }
        if let Some(f) = self.windowed {
            parts.push(format!("windowed={},{}", f.space, rect(f.frame)));
        }
        if let Some(z) = self.unzoomed {
            parts.push(format!("unzoomed={}", rect(z)));
        }
        if let Some(t) = self.tile {
            parts.push(format!("tile={t:?}"));
        }
        if let Some(u) = self.untiled {
            parts.push(format!("untiled={}", rect(u)));
        }
        if self.minimized {
            parts.push("minimized".into());
        }
        parts.join(";")
    }

    /// Takes back what [`Win::dump`] wrote; unknown parts are ignored.
    fn load(&mut self, data: &str) {
        let rect = |v: &str| {
            let n: Vec<i32> = v.split(',').filter_map(|p| p.parse().ok()).collect();
            (n.len() == 4).then(|| Rect::new(n[0], n[1], n[2], n[3]))
        };
        for part in data.split(';') {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            match key {
                "unzoomed" => self.unzoomed = rect(value),
                "untiled" => self.untiled = rect(value),
                "tile" => {
                    self.tile = [
                        Tile::Left,
                        Tile::Right,
                        Tile::TopLeft,
                        Tile::TopRight,
                        Tile::BottomLeft,
                        Tile::BottomRight,
                        Tile::Fill,
                    ]
                    .into_iter()
                    .find(|t| format!("{t:?}") == value);
                }
                "minimized" => self.minimized = true,
                "at" => {
                    if let Some((x, y)) = value.split_once(',')
                        && let (Ok(x), Ok(y)) = (x.parse(), y.parse())
                    {
                        self.x = x;
                        self.y = y;
                    }
                }
                "space" => self.space = value.parse().unwrap_or(0),
                "windowed" => {
                    if let Some((space, frame)) = value.split_once(',')
                        && let (Ok(space), Some(frame)) = (space.parse(), rect(frame))
                    {
                        self.windowed = Some(Windowed { space, frame });
                    }
                }
                _ => {}
            }
        }
    }

    /// The frame on screen (titlebar included).
    pub fn rect(&self) -> Rect {
        let (w, h) = self.size.unwrap_or((0, 0));
        let tb = if self.windowed.is_some() { 0 } else { self.tb };
        Rect::new(self.x, self.y, w, h + tb)
    }
}

/// The window manager's whole state.
#[derive(Debug, Default)]
pub struct Wm {
    pub outputs: BTreeMap<u32, Rect>,
    /// Each output's work area, once the core sent one.
    pub work: BTreeMap<u32, Rect>,
    pub windows: BTreeMap<u32, Win>,
    /// Visible windows, bottom to top.
    pub stack: Vec<u32>,
    pub focused: Option<u32>,
    /// Between bind and `done`: the core is describing the desktop.
    restoring: bool,
    /// A traffic light pressed and not yet released: (window, part). It
    /// acts on release over the same button, as buttons do.
    pressed: Option<(u32, u32)>,
    /// While a window is dragged: the tile the pointer is over, and the
    /// screen it is on (its preview is showing).
    snap: Option<(Tile, Rect)>,
    /// The Spaces, left to right, and which one is on screen.
    pub spaces: Vec<SpaceInfo>,
    pub current: usize,
    next_space: u32,
    /// What the core holds for us (`save_wm`).
    saved_desktop: String,
    /// Overview, while it is up.
    pub overview: Option<Overview>,
    /// The app switcher, while it is up.
    pub switcher: Option<Switcher>,
    /// Windows by when they last had focus (most recent last).
    pub mru: Vec<u32>,
}

impl Wm {
    pub fn new() -> Self {
        Wm {
            restoring: true,
            spaces: vec![SpaceInfo {
                id: 1,
                fullscreen: None,
            }],
            next_space: 2,
            ..Wm::default()
        }
    }

    /// Handles one event; returns what to ask of the core.
    // One arm per event: a table, long by nature.
    #[allow(clippy::too_many_lines)]
    pub fn handle(&mut self, ev: Event) -> Vec<Cmd> {
        let mut out = Vec::new();
        match ev {
            Event::Output { id, x, y, w, h } => {
                self.outputs.insert(id, Rect::new(x, y, w, h));
            }
            Event::OutputRemoved(id) => {
                self.outputs.remove(&id);
                self.work.remove(&id);
            }
            Event::WorkArea { id, x, y, w, h } => {
                let area = Rect::new(x, y, w, h);
                if self.work.insert(id, area) != Some(area) {
                    self.refit(&mut out);
                }
            }
            Event::Window {
                id,
                app_id,
                title,
                parent,
            } => {
                self.windows.insert(
                    id,
                    Win {
                        app_id,
                        title,
                        parent,
                        ..Win::default()
                    },
                );
            }
            Event::Title(id, t) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.title = t;
                }
            }
            Event::AppId(id, a) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.app_id = a;
                }
            }
            Event::Hints { id, min, max } => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.min = min;
                    w.max = max;
                }
            }
            Event::Size { id, w, h } => self.on_size(id, w, h, &mut out),
            Event::Closed(id) => self.on_closed(id, &mut out),
            Event::Request { id, kind, .. } => match kind {
                request::ACTIVATE => self.restore_and_focus(id, &mut out),
                request::MAXIMIZE => self.zoom(id, Some(true), &mut out),
                request::UNMAXIMIZE => self.zoom(id, Some(false), &mut out),
                request::MINIMIZE => self.minimize(id, &mut out),
                request::FULLSCREEN => self.fullscreen(id, Some(true), &mut out),
                request::UNFULLSCREEN => self.fullscreen(id, Some(false), &mut out),
                _ => {}
            },
            Event::Restore {
                id,
                x,
                y,
                w,
                h,
                states,
                visible,
                data,
            } => {
                if let Some(win) = self.windows.get_mut(&id) {
                    win.x = x;
                    win.y = y;
                    // A hidden window's saved place wins over where the
                    // core drew it last (slid off screen, shrunk away).
                    win.load(&data);
                    win.saved = data;
                    if w > 0 && h > 0 {
                        win.size = Some((w, h));
                    }
                    win.visible = visible;
                    win.states = states;
                    if !self.mru.contains(&id) {
                        self.mru.push(id);
                    }
                    if !win.minimized && !self.stack.contains(&id) {
                        self.stack.push(id);
                    }
                    if states & state::ACTIVATED != 0 {
                        self.focused = Some(id);
                    }
                }
            }
            Event::Icon { id, rect } => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.icon = rect;
                }
            }
            Event::RestoreWm(data) => {
                self.load_spaces(&data);
                self.saved_desktop = data;
            }
            Event::Done => {
                self.restoring = false;
                // Windows from before Spaces were saved: the one on screen.
                let here = self.current_space();
                for w in self.windows.values_mut() {
                    if !self.spaces.iter().any(|s| s.id == w.space) {
                        w.space = here;
                    }
                }
                for &(id, mods, keysym) in keys::BINDINGS {
                    out.push(Cmd::Bind { id, mods, keysym });
                }
                out.push(Cmd::Stack(self.stack.clone()));
                out.push(Cmd::Focus(self.focused.unwrap_or(0)));
            }
            Event::Decoration {
                id,
                server_side,
                titlebar,
            } => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.tb = if server_side { titlebar } else { 0 };
                    if server_side {
                        out.push(Cmd::Decoration { id, titlebar: true });
                    }
                }
            }
            Event::Button { x, y, pressed, .. } if self.overview.is_some() => {
                self.overview_click(x, y, pressed, &mut out);
            }
            Event::Button {
                window,
                part,
                pressed,
                clicks,
                edges,
                ..
            } => self.on_button(window, part, pressed, clicks, edges, &mut out),
            Event::MoveUpdate { id, x, y, px, py } => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.x = x;
                    w.y = y;
                }
                self.drag_update(id, px, py, &mut out);
            }
            Event::MoveEnd { id, x, y, .. } => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.x = x;
                    w.y = y;
                }
                self.drag_end(id, &mut out);
            }
            Event::ResizeEnd { id, x, y, w, h } => {
                if let Some(win) = self.windows.get_mut(&id) {
                    win.x = x;
                    win.y = y;
                    win.size = Some((w, h));
                    win.cfg = (w, h);
                    // Resized by hand: no longer tiled.
                    if win.tile.take().is_some() {
                        win.untiled = None;
                        win.states &= !(state::TILED_LEFT
                            | state::TILED_RIGHT
                            | state::TILED_TOP
                            | state::TILED_BOTTOM);
                        out.push(Cmd::Configure {
                            id,
                            w,
                            h,
                            states: win.states,
                        });
                    }
                }
            }
            Event::Binding(id) => {
                let key = match id {
                    keys::TILE_LEFT => TileKey::Left,
                    keys::TILE_RIGHT => TileKey::Right,
                    keys::TILE_UP => TileKey::Up,
                    keys::TILE_DOWN => TileKey::Down,
                    keys::SPACE_LEFT => {
                        self.switch_by(-1, &mut out);
                        return self.finish(out);
                    }
                    keys::SPACE_RIGHT => {
                        self.switch_by(1, &mut out);
                        return self.finish(out);
                    }
                    keys::CARRY_LEFT => {
                        self.carry_by(-1, &mut out);
                        return self.finish(out);
                    }
                    keys::CARRY_RIGHT => {
                        self.carry_by(1, &mut out);
                        return self.finish(out);
                    }
                    keys::OVERVIEW => {
                        self.toggle_overview(&mut out);
                        return self.finish(out);
                    }
                    keys::SWITCH_APPS | keys::SWITCH_APPS_BACK => {
                        self.switcher_step(id == keys::SWITCH_APPS_BACK, &mut out);
                        return self.finish(out);
                    }
                    keys::CYCLE_WINDOWS => {
                        self.cycle_app_windows(&mut out);
                        return self.finish(out);
                    }
                    keys::MINIMIZE => {
                        if let Some(id) = self.focused {
                            self.minimize(id, &mut out);
                        }
                        return self.finish(out);
                    }
                    keys::FULLSCREEN => {
                        if let Some(id) = self.focused {
                            self.fullscreen(id, None, &mut out);
                        }
                        return self.finish(out);
                    }
                    _ => return out,
                };
                self.tile_key(key, &mut out);
            }
            Event::Motion { x, y } => self.overview_hover(x, y, &mut out),
            Event::Key {
                keysym,
                pressed,
                mods: held,
            } => {
                if self.switcher.is_some() {
                    self.switcher_key(keysym, pressed, held, &mut out);
                } else if self.overview.is_some() {
                    self.overview_key(keysym, pressed, held, &mut out);
                }
            }
        }
        self.finish(out)
    }

    /// Ends handling an event: saves what changed.
    fn finish(&mut self, mut out: Vec<Cmd>) -> Vec<Cmd> {
        // Overview follows windows coming and going.
        if let Some(o) = &self.overview {
            let mut shown: Vec<u32> = self
                .windows
                .iter()
                .filter(|(_, w)| w.visible && w.mapped())
                .map(|(i, _)| *i)
                .collect();
            let mut tiled: Vec<u32> = o.tiles.iter().map(|t| t.0).collect();
            shown.sort_unstable();
            tiled.sort_unstable();
            if shown != tiled {
                self.layout_overview(&mut out);
            }
        }
        if !self.restoring {
            self.save_changes(&mut out);
        }
        out
    }

    /// Hands the core what a restarted window manager will need for each
    /// window whose memory changed (zoomed, tiled, minimized frames).
    fn save_changes(&mut self, out: &mut Vec<Cmd>) {
        for (&id, win) in &mut self.windows {
            let data = win.dump();
            if data != win.saved {
                out.push(Cmd::Save(id, data.clone()));
                win.saved = data;
            }
        }
        let data = self.dump_spaces();
        if data != self.saved_desktop {
            out.push(Cmd::SaveWm(data.clone()));
            self.saved_desktop = data;
        }
    }

    /// The output a window is on (the one it overlaps most), or the first.
    pub fn output_of(&self, r: &Rect) -> Rect {
        self.outputs
            .values()
            .max_by_key(|o| o.overlap(r))
            .copied()
            .unwrap_or(Rect::new(0, 0, 1280, 800))
    }

    fn on_button(
        &mut self,
        window: u32,
        part: u32,
        pressed: bool,
        clicks: u32,
        edges: u32,
        out: &mut Vec<Cmd>,
    ) {
        if !pressed {
            // A traffic light acts when released over itself.
            if let Some((w, p)) = self.pressed.take()
                && (w, p) == (window, part)
            {
                match p {
                    part::CLOSE => out.push(Cmd::Close(w)),
                    part::MINIMIZE => self.minimize(w, out),
                    part::ZOOM => self.zoom(w, None, out),
                    _ => {}
                }
            }
            return;
        }
        if window == 0 {
            return;
        }
        match part {
            part::CLOSE | part::MINIMIZE | part::ZOOM => {
                self.pressed = Some((window, part));
            }
            part::TITLEBAR => {
                self.focus(Some(window), out);
                if clicks >= 2 {
                    self.zoom(window, None, out);
                } else {
                    out.push(Cmd::BeginMove(window));
                }
            }
            part::EDGE => {
                self.focus(Some(window), out);
                out.push(Cmd::BeginResize(window, edges));
            }
            _ => self.focus(Some(window), out),
        }
    }

    /// The area windows may use on the output holding `r`: the output
    /// without the menu bar and dock.
    pub fn work_area(&self, r: &Rect) -> Rect {
        self.outputs
            .iter()
            .max_by_key(|(_, o)| o.overlap(r))
            .map_or(Rect::new(0, 0, 1280, 800), |(id, o)| {
                self.work.get(id).copied().unwrap_or(*o)
            })
    }

    /// The work area changed (the dock grew, the menu bar appeared): tiled
    /// and zoomed windows on screen follow it.
    fn refit(&mut self, out: &mut Vec<Cmd>) {
        let ids: Vec<u32> = self
            .windows
            .iter()
            .filter(|(_, w)| w.visible && !w.minimized && w.windowed.is_none())
            .filter(|(_, w)| w.tile.is_some() || w.unzoomed.is_some())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let Some(win) = self.windows.get(&id) else {
                continue;
            };
            let area = self.work_area(&win.rect());
            if let Some(t) = win.tile {
                self.tile(id, t, area, out);
                continue;
            }
            let Some(win) = self.windows.get_mut(&id) else {
                continue;
            };
            win.cfg = (area.w, area.h - win.tb);
            win.x = area.x;
            win.y = area.y;
            out.push(Cmd::Configure {
                id,
                w: win.cfg.0,
                h: win.cfg.1,
                states: win.states,
            });
            out.push(Cmd::Place {
                id,
                x: area.x,
                y: area.y,
                scale: 1.0,
                alpha: 1.0,
                motion: motion::SMOOTH,
            });
        }
    }

    /// Zoom: fills the work area, or returns to the frame it had (`None`
    /// toggles). The content gets the room left under the titlebar.
    pub fn zoom(&mut self, id: u32, on: Option<bool>, out: &mut Vec<Cmd>) {
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        let zoomed = win.unzoomed.is_some();
        let on = on.unwrap_or(!zoomed);
        if on == zoomed || win.windowed.is_some() {
            return;
        }
        // A tiled window zooms from, and returns to, its free frame.
        let frame = match (on, win.tile, win.untiled) {
            (true, Some(_), Some(u)) => Rect::new(u.x, u.y, u.w, u.h + win.tb),
            _ => win.rect(),
        };
        let target = if on {
            self.work_area(&frame)
        } else {
            win.unzoomed.unwrap_or(frame)
        };
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        win.unzoomed = on.then_some(frame);
        win.tile = None;
        win.untiled = None;
        win.states &=
            !(state::TILED_LEFT | state::TILED_RIGHT | state::TILED_TOP | state::TILED_BOTTOM);
        win.states = if on {
            win.states | state::MAXIMIZED
        } else {
            win.states & !state::MAXIMIZED
        };
        win.cfg = (target.w, target.h - win.tb);
        win.x = target.x;
        win.y = target.y;
        out.push(Cmd::Configure {
            id,
            w: win.cfg.0,
            h: win.cfg.1,
            states: win.states,
        });
        out.push(Cmd::Place {
            id,
            x: target.x,
            y: target.y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH,
        });
    }

    /// Minimize: shrinks and fades into its dock icon (or, with no dock,
    /// towards the bottom middle of the screen), then hides; focus passes
    /// to the next window. Fullscreen windows do not minimize.
    pub fn minimize(&mut self, id: u32, out: &mut Vec<Cmd>) {
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        if win.minimized || !win.visible || win.windowed.is_some() {
            return;
        }
        let frame = win.rect();
        let screen = self.output_of(&frame);
        let target = win.icon.unwrap_or_else(|| {
            let w = frame.w * 12 / 100;
            let h = frame.h * 12 / 100;
            Rect::new(
                screen.x + (screen.w - w) / 2,
                screen.y + screen.h - h - 8,
                w,
                h,
            )
        });
        let (x, y, scale) = shrink_into(frame, target);
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        win.minimized = true;
        win.visible = false;
        out.push(Cmd::Place {
            id,
            x,
            y,
            scale,
            alpha: 0.0,
            motion: motion::GENTLE,
        });
        out.push(Cmd::Visible { id, visible: false });
        self.stack.retain(|&i| i != id);
        out.push(Cmd::Stack(self.stack.clone()));
        let was_focused = self.focused == Some(id);
        self.set_states(id, |s| (s & !state::ACTIVATED) | state::MINIMIZED, out);
        if was_focused {
            self.focused = None;
            let next = self.top_shown();
            self.focus(next, out);
        }
    }

    /// Brings a window to the front: from its Space (which comes on
    /// screen) and out of the dock if minimized.
    pub fn restore_and_focus(&mut self, id: u32, out: &mut Vec<Cmd>) {
        if let Some(space) = self.windows.get(&id).map(|w| w.space)
            && space != self.current_space()
            && let Some(to) = self.spaces.iter().position(|s| s.id == space)
        {
            self.switch_space(to, None, out);
        }
        if let Some(win) = self.windows.get_mut(&id)
            && win.minimized
        {
            win.minimized = false;
            win.visible = true;
            win.states &= !state::MINIMIZED;
            out.push(Cmd::Visible { id, visible: true });
            out.push(Cmd::Place {
                id,
                x: win.x,
                y: win.y,
                scale: 1.0,
                alpha: 1.0,
                motion: motion::GENTLE,
            });
        }
        self.focus(Some(id), out);
    }

    fn on_size(&mut self, id: u32, w: i32, h: i32, out: &mut Vec<Cmd>) {
        let first = match self.windows.get_mut(&id) {
            Some(win) => {
                let first = !win.mapped();
                win.size = Some((w, h));
                first
            }
            None => return,
        };
        // While the core describes the desktop after a restart, sizes are
        // facts about placed windows (restore follows), not new windows.
        if !first || self.restoring {
            return;
        }
        let area = self
            .outputs
            .iter()
            .next()
            .map_or(Rect::new(0, 0, 1280, 800), |(id, o)| {
                self.work.get(id).copied().unwrap_or(*o)
            });
        let others: Vec<Rect> = self
            .stack
            .iter()
            .filter_map(|i| self.windows.get(i))
            .filter(|w| w.visible)
            .map(Win::rect)
            .collect();
        let tb = self.windows.get(&id).map_or(0, |w| w.tb);
        let (x, y) = place(area, (w, h + tb), &others);
        let here = self.current_space();
        if let Some(win) = self.windows.get_mut(&id) {
            win.x = x;
            win.y = y;
            win.visible = true;
            win.space = here;
        }
        // Opening: from a little lower, smaller and transparent, up into
        // place on the smooth spring.
        out.push(Cmd::Place {
            id,
            x,
            y: y + OPEN_DROP,
            scale: OPEN_SCALE,
            alpha: 0.0,
            motion: motion::INSTANT,
        });
        out.push(Cmd::Place {
            id,
            x,
            y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH,
        });
        out.push(Cmd::Visible { id, visible: true });
        self.stack.push(id);
        self.focus(Some(id), out);
    }

    fn on_closed(&mut self, id: u32, out: &mut Vec<Cmd>) {
        if self
            .switcher
            .as_ref()
            .is_some_and(|s| s.apps.iter().any(|a| a.1 == id))
        {
            // What it shows is out of date: close it, nothing chosen.
            self.switcher = None;
            out.push(Cmd::OverlayClear);
            out.push(Cmd::GrabKeyboard(false));
        }
        self.closing(id, out);
        self.mru.retain(|&i| i != id);
        self.windows.remove(&id);
        self.stack.retain(|&i| i != id);
        if self.focused == Some(id) {
            self.focused = None;
            let next = self.top_shown();
            self.focus(next, out);
        }
    }

    /// Focuses and raises `id` (or nobody), updating activated states.
    pub fn focus(&mut self, id: Option<u32>, out: &mut Vec<Cmd>) {
        let id = id.filter(|i| self.windows.get(i).is_some_and(|w| w.visible));
        if let Some(prev) = self.focused
            && Some(prev) != id
        {
            self.set_states(prev, |s| s & !state::ACTIVATED, out);
        }
        self.focused = id;
        if let Some(id) = id {
            self.mru.retain(|&i| i != id);
            self.mru.push(id);
            self.set_states(id, |s| s | state::ACTIVATED, out);
            self.stack.retain(|&i| i != id);
            self.stack.push(id);
            out.push(Cmd::Stack(self.stack.clone()));
        }
        out.push(Cmd::Focus(id.unwrap_or(0)));
    }

    fn set_states(&mut self, id: u32, f: impl Fn(u32) -> u32, out: &mut Vec<Cmd>) {
        if let Some(w) = self.windows.get_mut(&id) {
            let s = f(w.states);
            if s != w.states || !self.restoring {
                w.states = s;
                out.push(Cmd::Configure {
                    id,
                    w: w.cfg.0,
                    h: w.cfg.1,
                    states: s,
                });
            }
        }
    }
}

/// Where a frame shrunk into `target` is placed, and its scale: the
/// largest size that fits, centred on it (scaling keeps the top-left).
pub fn shrink_into(frame: Rect, target: Rect) -> (i32, i32, f64) {
    let scale = (f64::from(target.w) / f64::from(frame.w.max(1)))
        .min(f64::from(target.h) / f64::from(frame.h.max(1)))
        .clamp(0.01, 1.0);
    let cx = f64::from(target.x) + f64::from(target.w) / 2.0;
    let cy = f64::from(target.y) + f64::from(target.h) / 2.0;
    #[allow(clippy::cast_possible_truncation)]
    let at = |c: f64, len: i32| (c - f64::from(len) * scale / 2.0).round() as i32;
    (at(cx, frame.w), at(cy, frame.h), scale)
}

/// Where a new window of `size` goes in `area`: centred, a little above
/// the middle, unless that covers other windows; then the spot that
/// covers them least (ties: the one nearest that first choice).
pub fn place(area: Rect, size: (i32, i32), others: &[Rect]) -> (i32, i32) {
    let (width, height) = size;
    let base = (
        area.x + ((area.w - width) / 2).max(0),
        area.y + ((area.h - height) * 2 / 5).max(0),
    );
    let cost = |px: i32, py: i32| -> i64 {
        let r = Rect::new(px, py, width, height);
        others.iter().map(|o| r.overlap(o)).sum()
    };
    if cost(base.0, base.1) == 0 {
        return base;
    }
    let max_x = area.x + (area.w - width).max(0);
    let max_y = area.y + (area.h - height).max(0);
    // Candidates: a grid, plus every position flush against another
    // window or the area's edges (where the gaps are).
    let step = 24;
    let mut xs: Vec<i32> = (area.x..=max_x).step_by(step).collect();
    let mut ys: Vec<i32> = (area.y..=max_y).step_by(step).collect();
    xs.extend([area.x, max_x, base.0]);
    ys.extend([area.y, max_y, base.1]);
    for o in others {
        xs.extend([o.x + o.w, o.x - width]);
        ys.extend([o.y + o.h, o.y - height]);
    }
    xs.retain(|x| (area.x..=max_x).contains(x));
    ys.retain(|y| (area.y..=max_y).contains(y));
    let mut best = (i64::MAX, i64::MAX, base);
    for &py in &ys {
        for &px in &xs {
            let covered = cost(px, py);
            let dist = i64::from((px - base.0).abs()) + i64::from((py - base.1).abs());
            if (covered, dist) < (best.0, best.1) {
                best = (covered, dist, (px, py));
            }
        }
    }
    best.2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wm_with_output() -> Wm {
        let mut wm = Wm::new();
        wm.handle(Event::Output {
            id: 1,
            x: 0,
            y: 0,
            w: 2560,
            h: 1440,
        });
        wm.handle(Event::Done);
        wm
    }

    fn last_focus(cmds: &[Cmd]) -> Option<&Cmd> {
        cmds.iter().rev().find(|c| matches!(c, Cmd::Focus(_)))
    }

    fn open(wm: &mut Wm, id: u32, w: i32, h: i32) -> Vec<Cmd> {
        wm.handle(Event::Window {
            id,
            app_id: "app".into(),
            title: "t".into(),
            parent: 0,
        });
        wm.handle(Event::Size { id, w, h })
    }

    #[test]
    fn first_window_is_centred_a_little_high_and_focused() {
        let mut wm = wm_with_output();
        let cmds = open(&mut wm, 1, 800, 600);
        // It springs up into place from a little lower, smaller, invisible.
        assert!(cmds.contains(&Cmd::Place {
            id: 1,
            x: 880,
            y: 336 + OPEN_DROP,
            scale: OPEN_SCALE,
            alpha: 0.0,
            motion: motion::INSTANT
        }));
        assert!(cmds.contains(&Cmd::Place {
            id: 1,
            x: 880,
            y: 336,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH
        }));
        assert!(cmds.contains(&Cmd::Visible {
            id: 1,
            visible: true
        }));
        assert!(cmds.contains(&Cmd::Configure {
            id: 1,
            w: 0,
            h: 0,
            states: state::ACTIVATED
        }));
        assert_eq!(last_focus(&cmds), Some(&Cmd::Focus(1)));
    }

    #[test]
    fn a_second_window_avoids_the_first() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 800, 600);
        open(&mut wm, 2, 800, 600);
        let (a, b) = (wm.windows[&1].rect(), wm.windows[&2].rect());
        assert_eq!(a.overlap(&b), 0, "{a:?} {b:?}");
    }

    #[test]
    fn focus_moves_activation_and_raises() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        open(&mut wm, 2, 400, 300);
        assert_eq!(wm.stack, [1, 2]);
        let cmds = wm.handle(Event::Button {
            window: 1,
            part: part::CONTENT,
            x: 0.0,
            y: 0.0,
            button: 0x110,
            pressed: true,
            mods: 0,
            clicks: 1,
            edges: 0,
        });
        assert!(cmds.contains(&Cmd::Configure {
            id: 2,
            w: 0,
            h: 0,
            states: 0
        }));
        assert!(cmds.contains(&Cmd::Configure {
            id: 1,
            w: 0,
            h: 0,
            states: state::ACTIVATED
        }));
        assert!(cmds.contains(&Cmd::Stack(vec![2, 1])));
        assert_eq!(wm.focused, Some(1));
    }

    #[test]
    fn closing_the_focused_window_focuses_the_next_one_down() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        open(&mut wm, 2, 400, 300);
        let cmds = wm.handle(Event::Closed(2));
        assert_eq!(wm.focused, Some(1));
        assert_eq!(last_focus(&cmds), Some(&Cmd::Focus(1)));
    }

    #[test]
    fn a_restarted_wm_takes_over_the_desktop_as_it_was() {
        let mut wm = Wm::new();
        wm.handle(Event::Output {
            id: 1,
            x: 0,
            y: 0,
            w: 1280,
            h: 800,
        });
        for (id, x, active) in [(4, 10, false), (7, 300, true)] {
            wm.handle(Event::Window {
                id,
                app_id: "a".into(),
                title: "t".into(),
                parent: 0,
            });
            wm.handle(Event::Size { id, w: 200, h: 100 });
            wm.handle(Event::Restore {
                id,
                x,
                y: 20,
                w: 200,
                h: 100,
                states: if active { state::ACTIVATED } else { 0 },
                visible: true,
                data: String::new(),
            });
        }
        let cmds = wm.handle(Event::Done);
        let at = cmds.iter().position(|c| *c == Cmd::Stack(vec![4, 7]));
        assert!(at.is_some_and(|i| cmds.get(i + 1) == Some(&Cmd::Focus(7))));
        assert_eq!(wm.windows[&4].rect(), Rect::new(10, 20, 200, 100));
    }

    #[test]
    fn placement_prefers_the_least_covered_spot_nearest_the_centre() {
        let area = Rect::new(0, 0, 1000, 800);
        assert_eq!(place(area, (400, 300), &[]), (300, 200));
        let taken = [Rect::new(300, 200, 400, 300)];
        let p = place(area, (400, 300), &taken);
        assert_eq!(Rect::new(p.0, p.1, 400, 300).overlap(&taken[0]), 0);
    }

    #[test]
    fn what_a_window_manager_remembers_survives_a_restart() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        let free = wm.windows[&1].rect();
        let cmds = wm.handle(Event::Binding(keys::TILE_LEFT));
        let Some(Cmd::Save(1, data)) = cmds.iter().find(|c| matches!(c, Cmd::Save(..))) else {
            panic!("tiling saved nothing: {cmds:?}");
        };
        assert_eq!(
            data,
            &format!("space=1;tile=Left;untiled={},{},400,300", free.x, free.y)
        );
        let mut back = Win {
            visible: true,
            ..Win::default()
        };
        back.load(data);
        assert_eq!(back.tile, Some(Tile::Left));
        assert_eq!(back.untiled, Some(Rect::new(free.x, free.y, 400, 300)));
        assert_eq!(back.dump(), *data);
    }

    #[test]
    fn super_arrows_tile_and_down_goes_back() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        let free = wm.windows[&1].rect();
        wm.handle(Event::Binding(keys::TILE_RIGHT));
        assert_eq!(wm.windows[&1].tile, Some(Tile::Right));
        assert_eq!(
            (wm.windows[&1].x, wm.windows[&1].y),
            (1280 + tile::GAP / 2, tile::GAP)
        );
        wm.handle(Event::Binding(keys::TILE_UP));
        assert_eq!(wm.windows[&1].tile, Some(Tile::TopRight));
        // Down from a quarter: back to the free frame.
        let cmds = wm.handle(Event::Binding(keys::TILE_DOWN));
        assert!(cmds.contains(&Cmd::Place {
            id: 1,
            x: free.x,
            y: free.y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH
        }));
        assert_eq!(wm.windows[&1].tile, None);
        assert_eq!(wm.windows[&1].cfg, (400, 300));
    }

    #[test]
    fn the_work_area_keeps_windows_off_the_menu_bar_and_dock() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        wm.handle(Event::Binding(keys::TILE_LEFT));
        let full = Tile::Left.rect(Rect::new(0, 0, 2560, 1440));
        assert_eq!((wm.windows[&1].x, wm.windows[&1].y), (full.x, full.y));
        // The shell takes 28 px at the top and 90 at the bottom: the tiled
        // window moves and shrinks with it.
        let work = Rect::new(0, 28, 2560, 1440 - 28 - 90);
        let cmds = wm.handle(Event::WorkArea {
            id: 1,
            x: work.x,
            y: work.y,
            w: work.w,
            h: work.h,
        });
        let left = Tile::Left.rect(work);
        assert!(
            cmds.iter().any(
                |c| matches!(c, Cmd::Place { id: 1, x, y, .. } if (*x, *y) == (left.x, left.y))
            )
        );
        assert_eq!(wm.windows[&1].cfg.1, left.h - wm.windows[&1].tb);
        // The same area again changes nothing.
        let again = wm.handle(Event::WorkArea {
            id: 1,
            x: work.x,
            y: work.y,
            w: work.w,
            h: work.h,
        });
        assert!(!again.iter().any(|c| matches!(c, Cmd::Place { .. })));
        // New windows are placed inside it.
        open(&mut wm, 2, 2000, 1300);
        let r = wm.windows[&2].rect();
        assert!(r.y >= work.y, "{r:?} under the menu bar");
    }

    #[test]
    fn dragging_to_an_edge_previews_then_snaps() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        let cmds = wm.handle(Event::MoveUpdate {
            id: 1,
            x: 100,
            y: 400,
            px: 0.0,
            py: 700.0,
        });
        let left = Tile::Left.rect(Rect::new(0, 0, 2560, 1440));
        assert!(cmds.contains(&Cmd::OverlayRect {
            x: left.x,
            y: left.y,
            w: left.w,
            h: left.h,
            radius: 12,
            rgba: 0x7C_C4_8A_40
        }));
        // Moving within the zone sends nothing more.
        let again = wm.handle(Event::MoveUpdate {
            id: 1,
            x: 100,
            y: 410,
            px: 0.0,
            py: 710.0,
        });
        assert!(!again.iter().any(|c| matches!(c, Cmd::OverlayRect { .. })));
        let cmds = wm.handle(Event::MoveEnd {
            id: 1,
            x: 100,
            y: 410,
            px: 0.0,
            py: 710.0,
        });
        assert!(cmds.contains(&Cmd::OverlayClear));
        assert!(cmds.contains(&Cmd::Configure {
            id: 1,
            w: left.w,
            h: left.h,
            states: state::ACTIVATED
                | state::TILED_LEFT
                | state::TILED_RIGHT
                | state::TILED_TOP
                | state::TILED_BOTTOM
        }));
        // The client takes the tiled size.
        wm.handle(Event::Size {
            id: 1,
            w: left.w,
            h: left.h,
        });
        // Dragged away: its own size back (the core keeps it in hand).
        let cmds = wm.handle(Event::MoveUpdate {
            id: 1,
            x: left.x + 50,
            y: left.y + 200,
            px: 600.0,
            py: 300.0,
        });
        assert!(cmds.contains(&Cmd::Configure {
            id: 1,
            w: 400,
            h: 300,
            states: state::ACTIVATED
        }));
        assert_eq!(wm.windows[&1].tile, None);
    }

    #[test]
    fn spaces_switch_carry_and_fullscreen() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        open(&mut wm, 2, 400, 300);
        // Carrying window 2 right makes Space 2; window 1 slides left out.
        let cmds = wm.handle(Event::Binding(keys::CARRY_RIGHT));
        assert_eq!(wm.spaces.len(), 2);
        assert_eq!(wm.current, 1);
        let x1 = wm.windows[&1].x;
        assert!(cmds.contains(&Cmd::Place {
            id: 1,
            x: x1 - 2560,
            y: wm.windows[&1].y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH
        }));
        assert!(cmds.contains(&Cmd::Visible {
            id: 1,
            visible: false
        }));
        assert!(cmds.iter().any(|c| matches!(c, Cmd::SaveWm(_))));
        assert_eq!(wm.focused, Some(2));
        // Fullscreen from Space 2 goes to a Space right after it; when it
        // ends the fullscreen Space goes and Space 2 is still there.
        wm.handle(Event::Binding(keys::FULLSCREEN));
        assert_eq!(wm.spaces.len(), 3);
        assert_eq!(wm.spaces[2].fullscreen, Some(2));
        assert_eq!(wm.current, 2);
        assert_eq!(wm.windows[&2].cfg, (2560, 1440));
        wm.handle(Event::Binding(keys::FULLSCREEN));
        assert_eq!(wm.spaces.len(), 2);
        assert_eq!(wm.current, 1);
        assert_eq!(wm.windows[&2].cfg, (400, 300));
        // Back to Space 1: its window gets focus; Space 2 keeps window 2.
        wm.handle(Event::Binding(keys::SPACE_LEFT));
        assert_eq!(wm.focused, Some(1));
        // The Spaces survive a restart through save_wm.
        let saved = wm.dump_spaces();
        let mut again = Wm::new();
        again.load_spaces(&saved);
        assert_eq!(again.spaces, wm.spaces);
        assert_eq!(again.current, wm.current);
    }

    #[test]
    fn minimize_shrinks_into_the_dock_icon_and_comes_back() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        open(&mut wm, 2, 400, 300);
        let icon = Rect::new(1000, 1380, 48, 48);
        wm.handle(Event::Icon {
            id: 2,
            rect: Some(icon),
        });
        let before = wm.windows[&2].rect();
        let cmds = wm.handle(Event::Binding(keys::MINIMIZE));
        let place = cmds.iter().find_map(|c| match c {
            Cmd::Place {
                id: 2,
                x,
                y,
                scale,
                alpha,
                motion: spring,
            } => Some((*x, *y, *scale, *alpha, *spring)),
            _ => None,
        });
        let (left, top, scale, alpha, spring) = place.expect("no placement for the minimize");
        assert_eq!((alpha, spring), (0.0, motion::GENTLE));
        // Shrunk to fit the icon, centred on it.
        let width = f64::from(before.w) * scale;
        let height = f64::from(before.h) * scale;
        assert!(width <= 48.5 && height <= 48.5, "{width}x{height}");
        assert!((f64::from(left) + width / 2.0 - 1024.0).abs() <= 1.0);
        assert!((f64::from(top) + height / 2.0 - 1404.0).abs() <= 1.0);
        // The dock sees it minimized; focus moved to window 1.
        assert!(cmds.contains(&Cmd::Configure {
            id: 2,
            w: 0,
            h: 0,
            states: state::MINIMIZED
        }));
        assert_eq!(wm.focused, Some(1));
        // Activated from the dock: back where it was, focused, not
        // minimized.
        let cmds = wm.handle(Event::Request {
            id: 2,
            kind: request::ACTIVATE,
            edges: 0,
        });
        assert!(cmds.contains(&Cmd::Place {
            id: 2,
            x: before.x,
            y: before.y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::GENTLE
        }));
        assert_eq!(wm.focused, Some(2));
        assert_eq!(wm.windows[&2].states, state::ACTIVATED);
    }

    #[test]
    fn overview_tiles_the_windows_and_a_click_brings_one_forward() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 800, 600);
        open(&mut wm, 2, 800, 600);
        let homes = (wm.windows[&1].rect(), wm.windows[&2].rect());
        let cmds = wm.handle(Event::Binding(keys::OVERVIEW));
        assert!(cmds.contains(&Cmd::GrabPointer(true)));
        assert!(cmds.contains(&Cmd::GrabKeyboard(true)));
        let o = wm.overview.clone().expect("no overview");
        assert_eq!(o.tiles.len(), 2);
        assert_eq!(o.thumbs.len(), 1);
        // The windows themselves go to the tiles; the strip is overlay.
        for (id, r, scale) in &o.tiles {
            assert!(cmds.contains(&Cmd::Place {
                id: *id,
                x: r.x,
                y: r.y,
                scale: *scale,
                alpha: 1.0,
                motion: motion::SMOOTH
            }));
        }
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Cmd::OverlayWallpaper { .. }))
        );
        assert!(cmds.contains(&Cmd::OverlayMirror {
            id: 1,
            x: o.thumbs[0].x + (homes.0.x * o.thumbs[0].h + 720) / 1440,
            y: o.thumbs[0].y + (homes.0.y * o.thumbs[0].h + 720) / 1440,
            w: (800 * o.thumbs[0].h + 720) / 1440,
            h: (600 * o.thumbs[0].h + 720) / 1440,
        }));
        // Clicking window 1's tile: Overview closes, everything goes
        // home, window 1 is in front.
        let (_, r1, _) = o.tiles.iter().find(|t| t.0 == 1).copied().unwrap();
        let click = |pressed| Event::Button {
            window: 0,
            part: part::OVERLAY,
            x: f64::from(r1.x + 5),
            y: f64::from(r1.y + 5),
            button: 0x110,
            pressed,
            mods: 0,
            clicks: 1,
            edges: 0,
        };
        wm.handle(click(true));
        let cmds = wm.handle(click(false));
        assert!(wm.overview.is_none());
        assert!(cmds.contains(&Cmd::GrabPointer(false)));
        assert!(cmds.contains(&Cmd::Place {
            id: 2,
            x: homes.1.x,
            y: homes.1.y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH
        }));
        assert_eq!(wm.focused, Some(1));
    }

    #[test]
    fn super_tab_picks_the_previous_app_and_letting_go_switches() {
        let mut wm = wm_with_output();
        let app = |wm: &mut Wm, id: u32, app: &str| {
            wm.handle(Event::Window {
                id,
                app_id: app.into(),
                title: app.into(),
                parent: 0,
            });
            wm.handle(Event::Size { id, w: 300, h: 200 });
        };
        app(&mut wm, 1, "org.fern.Files");
        app(&mut wm, 2, "org.fern.Terminal");
        app(&mut wm, 3, "org.fern.Terminal");
        assert_eq!(wm.focused, Some(3));
        let cmds = wm.handle(Event::Binding(keys::SWITCH_APPS));
        assert!(cmds.contains(&Cmd::GrabKeyboard(true)));
        let s = wm.switcher.clone().expect("no switcher");
        // One entry per app, most recent first; the previous app picked.
        assert_eq!(
            s.apps,
            [
                ("org.fern.Terminal".to_string(), 3),
                ("org.fern.Files".to_string(), 1)
            ]
        );
        assert_eq!(s.picked, 1);
        assert!(cmds.iter().any(|c| matches!(c,
            Cmd::OverlayText { text, .. } if text == "Files")));
        // Letting go of Super: Files comes forward.
        let cmds = wm.handle(Event::Key {
            keysym: 0xffeb,
            pressed: false,
            mods: 0,
        });
        assert!(wm.switcher.is_none());
        assert!(cmds.contains(&Cmd::GrabKeyboard(false)));
        assert_eq!(wm.focused, Some(1));
        // A quick tap (Super already up when the keyboard is taken) also
        // switches, back to the Terminal.
        wm.handle(Event::Binding(keys::SWITCH_APPS));
        wm.handle(Event::Key {
            keysym: 0,
            pressed: false,
            mods: 0,
        });
        assert_eq!(wm.focused, Some(3));
        // Super+` goes through the Terminal's windows.
        wm.handle(Event::Binding(keys::CYCLE_WINDOWS));
        assert_eq!(wm.focused, Some(2));
    }

    #[test]
    fn a_restart_keeps_where_hidden_windows_belong() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        open(&mut wm, 2, 400, 300);
        let home = wm.windows[&1].rect();
        // Window 2 goes to Space 2; window 1 slides off screen.
        let cmds = wm.handle(Event::Binding(keys::CARRY_RIGHT));
        let saved: Vec<(u32, String)> = cmds
            .iter()
            .filter_map(|c| match c {
                Cmd::Save(id, d) => Some((*id, d.clone())),
                _ => None,
            })
            .collect();
        let wm_data = cmds.iter().rev().find_map(|c| match c {
            Cmd::SaveWm(d) => Some(d.clone()),
            _ => None,
        });
        let data1 = &saved.iter().rev().find(|(id, _)| *id == 1).unwrap().1;
        // A new window manager, told what the core knows: window 1 is
        // hidden and was last drawn off screen.
        let mut again = Wm::new();
        again.handle(Event::Output {
            id: 1,
            x: 0,
            y: 0,
            w: 2560,
            h: 1440,
        });
        for (id, x, visible, data) in [
            (1, home.x - 2560, false, data1.clone()),
            (2, 0, true, String::from("space=2")),
        ] {
            again.handle(Event::Window {
                id,
                app_id: "a".into(),
                title: "t".into(),
                parent: 0,
            });
            again.handle(Event::Size { id, w: 400, h: 300 });
            again.handle(Event::Restore {
                id,
                x,
                y: home.y,
                w: 400,
                h: 300,
                states: 0,
                visible,
                data,
            });
        }
        again.handle(Event::RestoreWm(wm_data.unwrap_or_default()));
        again.handle(Event::Done);
        // Back to Space 1: window 1 slides in to where it belongs.
        let cmds = again.handle(Event::Binding(keys::SPACE_LEFT));
        assert!(
            cmds.contains(&Cmd::Place {
                id: 1,
                x: home.x,
                y: home.y,
                scale: 1.0,
                alpha: 1.0,
                motion: motion::SMOOTH
            }),
            "{cmds:?}"
        );
    }
}
