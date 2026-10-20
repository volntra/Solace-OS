//! The shell's surfaces. The menu bar, dock, panels and banners are
//! wlr-layer-shell surfaces, offered to trusted clients only (the parts of
//! the desktop the core starts itself): an app could otherwise cover the
//! screen or fake the menu bar. Their exclusive zones make the window
//! manager's work area. `fern_shell_v1` adds what layer-shell lacks: a
//! rounded blur behind a shell surface, and global shortcuts
//! (docs/protocols/shell.md).

use std::collections::{HashMap, VecDeque};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fern_wm_proto::shell::server::fern_shell_v1::{self, FernShellV1};
use smithay::{
    delegate_layer_shell,
    desktop::{LayerSurface, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::{
        Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
        backend::ClientId, protocol::wl_output::WlOutput, protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::{
        compositor::with_states,
        shell::wlr_layer::{
            KeyboardInteractivity, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData,
            WlrLayerShellHandler, WlrLayerShellState,
        },
    },
};

use smithay::reexports::calloop::{
    ping::make_ping,
    timer::{TimeoutAction, Timer},
};

use crate::state::{ClientState, Comp, DisconnectPing};

/// Starts the shell and returns our end of its connection.
pub type ShellLauncher = Box<dyn FnMut() -> std::io::Result<UnixStream>>;

/// Restarts allowed within [`RESTART_WINDOW`] before the core gives up.
const MAX_RESTARTS: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(30);
const RESTART_DELAY: Duration = Duration::from_millis(500);

/// A rounded rectangle of a shell surface to blur behind (surface-local).
#[derive(Debug, Default)]
pub struct ShellBlur(Mutex<Option<(Rectangle<i32, Logical>, i32)>>);

/// The blur `surface` asked for through `fern_shell_v1`, if any.
pub fn shell_blur(surface: &WlSurface) -> Option<(Rectangle<i32, Logical>, i32)> {
    with_states(surface, |states| {
        states.data_map.get::<ShellBlur>().and_then(|b| {
            *b.0.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        })
    })
}

/// The shell's connection(s) to `fern_shell_v1` and their key bindings.
#[derive(Default)]
pub struct Shell {
    globals: Vec<FernShellV1>,
    /// id -> (modifier bits, keysym, the resource that asked).
    bindings: HashMap<u32, (u32, u32, FernShellV1)>,
    /// The work area last sent to the window manager, per output id.
    pub work_areas: HashMap<u32, Rectangle<i32, Logical>>,
    /// Keyboard focus before an exclusive layer took it.
    pub(crate) focus_before: Option<WlSurface>,
    launcher: Option<ShellLauncher>,
    gone: Option<smithay::reexports::calloop::ping::Ping>,
    restarts: VecDeque<Instant>,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("bindings", &self.bindings.len())
            .field("work_areas", &self.work_areas)
            .field("launcher", &self.launcher.is_some())
            .finish_non_exhaustive()
    }
}

/// Layers drawn under the windows, and over them.
pub const LOWER: [Layer; 2] = [Layer::Background, Layer::Bottom];
pub const UPPER: [Layer; 2] = [Layer::Top, Layer::Overlay];

impl Comp {
    /// How to start the shell (menu bar, dock, ...); starts it now, and
    /// again whenever it exits (up to a point, like the window manager).
    ///
    /// # Errors
    /// The event loop refuses the restart source.
    pub fn set_shell_launcher(&mut self, launcher: ShellLauncher) -> Result<(), String> {
        let (ping, source) = make_ping().map_err(|e| format!("shell restart: {e}"))?;
        self.loop_handle
            .insert_source(source, |(), (), data| data.state.shell_gone())
            .map_err(|e| format!("shell restart: {}", e.error))?;
        self.shell.gone = Some(ping);
        self.shell.launcher = Some(launcher);
        self.start_shell();
        Ok(())
    }

    fn start_shell(&mut self) {
        let Some(launcher) = self.shell.launcher.as_mut() else {
            return;
        };
        match launcher() {
            Ok(stream) => {
                let data = Arc::new(ClientState {
                    trusted: true,
                    on_disconnect: self.shell.gone.clone().map(DisconnectPing),
                    ..ClientState::default()
                });
                if let Err(e) = self.display_handle.insert_client(stream, data) {
                    tracing::warn!("shell connection: {e}");
                }
            }
            Err(e) => tracing::warn!("could not start the shell: {e}"),
        }
    }

    fn shell_gone(&mut self) {
        tracing::warn!("the shell disconnected; windows stay where they are");
        let now = Instant::now();
        while self
            .shell
            .restarts
            .front()
            .is_some_and(|t| now.duration_since(*t) > RESTART_WINDOW)
        {
            self.shell.restarts.pop_front();
        }
        if self.shell.restarts.len() >= MAX_RESTARTS {
            tracing::error!("the shell keeps exiting; not restarting it");
            return;
        }
        self.shell.restarts.push_back(now);
        let started =
            self.loop_handle
                .insert_source(Timer::from_duration(RESTART_DELAY), |_, (), data| {
                    data.state.start_shell();
                    TimeoutAction::Drop
                });
        if let Err(e) = started {
            tracing::warn!("scheduling the shell restart: {}", e.error);
        }
    }

    /// Offers `fern_shell_v1` (to trusted clients only).
    pub fn init_shell(&mut self) {
        self.display_handle
            .create_global::<Comp, FernShellV1, ()>(1, ());
    }

    /// The output id the window manager knows `output` by.
    pub fn output_id(&self, output: &Output) -> u32 {
        self.space
            .outputs()
            .position(|o| o == output)
            .and_then(|i| u32::try_from(i + 1).ok())
            .unwrap_or(0)
    }

    /// The output a layer surface is on.
    fn layer_output(&self, surface: &WlSurface) -> Option<Output> {
        self.space
            .outputs()
            .find(|o| {
                layer_map_for_output(o)
                    .layer_for_surface(surface, WindowSurfaceType::ALL)
                    .is_some()
            })
            .cloned()
    }

    /// A commit on a layer surface (or one of its subsurfaces): the first
    /// one gets its configure; every one may move things around. Returns
    /// whether `surface` belonged to a layer surface.
    pub fn layer_commit(&mut self, surface: &WlSurface) -> bool {
        let Some(output) = self.layer_output(surface) else {
            return false;
        };
        let initial = with_states(surface, |states| {
            states.data_map.get::<LayerSurfaceData>().map(|d| {
                !d.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .initial_configure_sent
            })
        });
        {
            let mut map = layer_map_for_output(&output);
            map.arrange();
            if initial == Some(true)
                && let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            {
                layer.layer_surface().send_configure();
            }
        }
        self.layers_changed();
        true
    }

    /// An output appeared or changed size: the shell's surfaces on it are
    /// laid out again.
    pub fn output_changed(&mut self, output: &Output) {
        layer_map_for_output(output).arrange();
        self.layers_changed();
    }

    /// Layers came, went or changed: tell the window manager about the
    /// work area and settle the keyboard.
    pub fn layers_changed(&mut self) {
        let mut changed = Vec::new();
        for output in self.space.outputs() {
            let Some(geo) = self.space.output_geometry(output) else {
                continue;
            };
            let zone = layer_map_for_output(output).non_exclusive_zone();
            let area = Rectangle::new(geo.loc + zone.loc, zone.size);
            let id = self.output_id(output);
            if self.shell.work_areas.get(&id) != Some(&area) {
                changed.push((id, area));
            }
        }
        for (id, area) in changed {
            self.shell.work_areas.insert(id, area);
            self.wm_work_area(id, area);
        }
        self.settle_layer_focus();
        self.request_redraw();
    }

    /// The topmost layer surface that wants the keyboard to itself, on
    /// the top or overlay layer.
    fn exclusive_layer(&self) -> Option<WlSurface> {
        self.space.outputs().find_map(|o| {
            let map = layer_map_for_output(o);
            UPPER.iter().rev().find_map(|&l| {
                map.layers_on(l)
                    .rev()
                    .find(|s| {
                        s.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive
                    })
                    .map(|s| s.wl_surface().clone())
            })
        })
    }

    /// An exclusive layer has the keyboard while it is mapped; when it goes,
    /// the keyboard returns to where it was.
    fn settle_layer_focus(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let current = keyboard.current_focus();
        match self.exclusive_layer() {
            Some(layer) if current.as_ref() != Some(&layer) => {
                if !current.as_ref().is_some_and(|c| self.is_layer(c)) {
                    self.shell.focus_before = current;
                }
                keyboard.set_focus(self, Some(layer), SERIAL_COUNTER.next_serial());
            }
            Some(_) => {}
            None => {
                let on_layer = current.as_ref().is_some_and(|c| self.is_layer(c));
                let gone = current.as_ref().is_some_and(|c| !c.is_alive());
                if on_layer
                    && self.layer_keyboard(current.as_ref()) == KeyboardInteractivity::Exclusive
                    || gone
                {
                    let back = self.shell.focus_before.take().filter(Resource::is_alive);
                    keyboard.set_focus(self, back, SERIAL_COUNTER.next_serial());
                }
            }
        }
    }

    fn is_layer(&self, surface: &WlSurface) -> bool {
        self.layer_output(surface).is_some()
    }

    fn layer_keyboard(&self, surface: Option<&WlSurface>) -> KeyboardInteractivity {
        surface
            .and_then(|s| {
                self.space.outputs().find_map(|o| {
                    layer_map_for_output(o)
                        .layer_for_surface(s, WindowSurfaceType::TOPLEVEL)
                        .map(|l| l.cached_state().keyboard_interactivity)
                })
            })
            .unwrap_or(KeyboardInteractivity::None)
    }

    /// The layer surface on `layers` under `pos` (global), with where its
    /// surface tree is drawn from (global).
    pub fn layer_at(
        &self,
        pos: Point<f64, Logical>,
        layers: &[Layer],
    ) -> Option<(LayerSurface, Point<i32, Logical>)> {
        for output in self.space.outputs() {
            let Some(geo) = self.space.output_geometry(output) else {
                continue;
            };
            if !geo.to_f64().contains(pos) {
                continue;
            }
            let local = pos - geo.loc.to_f64();
            let map = layer_map_for_output(output);
            for &l in layers.iter().rev() {
                if let Some(layer) = map.layer_under(l, local)
                    && let Some(lg) = map.layer_geometry(layer)
                    && layer
                        .surface_under(local - lg.loc.to_f64(), WindowSurfaceType::ALL)
                        .is_some()
                {
                    return Some((layer.clone(), geo.loc + lg.loc));
                }
            }
        }
        None
    }

    /// The surface of a layer on `layers` under `pos` and its global
    /// position (for pointer focus).
    pub fn layer_surface_under(
        &self,
        pos: Point<f64, Logical>,
        layers: &[Layer],
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let (layer, origin) = self.layer_at(pos, layers)?;
        layer
            .surface_under(pos - origin.to_f64(), WindowSurfaceType::ALL)
            .map(|(s, p)| (s, (p + origin).to_f64()))
    }

    /// Where a layer surface is drawn from (global), for `set_rectangle`.
    pub fn layer_origin(&self, surface: &WlSurface) -> Option<Point<i32, Logical>> {
        self.space.outputs().find_map(|o| {
            let map = layer_map_for_output(o);
            let layer = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)?;
            let lg = map.layer_geometry(layer)?;
            Some(self.space.output_geometry(o)?.loc + lg.loc)
        })
    }

    /// A press on a layer surface that takes the keyboard on demand (a
    /// search field, a panel) gives it the keyboard.
    pub fn layer_clicked(&mut self, layer: &LayerSurface) {
        if layer.cached_state().keyboard_interactivity == KeyboardInteractivity::None {
            return;
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let current = keyboard.current_focus();
        if !current.as_ref().is_some_and(|c| self.is_layer(c)) {
            self.shell.focus_before = current;
        }
        keyboard.set_focus(
            self,
            Some(layer.wl_surface().clone()),
            SERIAL_COUNTER.next_serial(),
        );
    }

    /// The keyboard is on a layer surface that holds it to itself.
    pub fn keyboard_held_by_layer(&self) -> Option<WlSurface> {
        self.exclusive_layer()
    }

    /// Frame callbacks to every layer surface on `output`.
    pub fn layers_frame_shown(&self, output: &Output, now: std::time::Duration) {
        let map = layer_map_for_output(output);
        for layer in map.layers() {
            layer.send_frame(output, now, Some(std::time::Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
    }

    /// A shell key binding for this key, if any.
    pub fn shell_binding(&self, mods: u32, keysym: u32) -> Option<u32> {
        self.shell
            .bindings
            .iter()
            .find(|(_, (m, k, _))| *m == mods && *k == keysym)
            .map(|(id, _)| *id)
    }

    pub fn shell_key_binding(&self, id: u32) {
        if let Some((_, _, r)) = self.shell.bindings.get(&id) {
            r.key_binding(id);
        }
    }
}

impl WlrLayerShellHandler for Comp {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wl_output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned());
        let Some(output) = output else {
            tracing::warn!("a layer surface ({namespace}) with no output to go on");
            surface.send_close();
            return;
        };
        let mut map = layer_map_for_output(&output);
        if let Err(e) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            tracing::warn!("layer surface: {e}");
        }
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for output in &outputs {
            let mut map = layer_map_for_output(output);
            let found = map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();
            if let Some(layer) = found {
                map.unmap_layer(&layer);
            }
        }
        self.layers_changed();
    }
}

delegate_layer_shell!(Comp);

impl GlobalDispatch<FernShellV1, ()> for Comp {
    fn bind(
        state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<FernShellV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let r = data_init.init(resource, ());
        let (service, path) = state.active_app_menu();
        r.app_menu(service, path);
        state.shell.globals.push(r);
    }

    fn can_view(client: Client, _global_data: &()) -> bool {
        client.get_data::<ClientState>().is_some_and(|d| d.trusted)
    }
}

impl Dispatch<FernShellV1, ()> for Comp {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &FernShellV1,
        request: fern_shell_v1::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use fern_shell_v1::Request as R;
        match request {
            R::SetBlur {
                surface,
                x,
                y,
                width,
                height,
                radius,
            } => {
                let blur = (width > 0 && height > 0).then(|| {
                    (
                        Rectangle::new((x, y).into(), (width, height).into()),
                        radius.max(0),
                    )
                });
                set_shell_blur(&surface, blur);
                state.request_redraw();
            }
            R::UnsetBlur { surface } => {
                set_shell_blur(&surface, None);
                state.request_redraw();
            }
            R::SetWallpaper { path } => {
                state.wallpaper_request =
                    Some((!path.is_empty()).then(|| std::path::PathBuf::from(path)));
                state.request_redraw();
            }
            R::BindKey {
                id,
                modifiers,
                keysym,
            } => {
                state
                    .shell
                    .bindings
                    .insert(id, (modifiers, keysym, resource.clone()));
            }
            R::UnbindKey { id } => {
                state.shell.bindings.remove(&id);
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _client: ClientId, resource: &FernShellV1, _data: &()) {
        state.shell.globals.retain(|g| g != resource);
        state.shell.bindings.retain(|_, (_, _, r)| r != resource);
    }
}

impl Comp {
    /// Tells the shell where the active window's menus are.
    pub(crate) fn shell_app_menu(&self, service: &str, path: &str) {
        for g in &self.shell.globals {
            g.app_menu(service.to_owned(), path.to_owned());
        }
    }
}

fn set_shell_blur(surface: &WlSurface, blur: Option<(Rectangle<i32, Logical>, i32)>) {
    with_states(surface, |states| {
        let b = states.data_map.get_or_insert_threadsafe(ShellBlur::default);
        *b.0.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = blur;
    });
}
