// Adapted from Smithay's smallvil example (v0.7.0, MIT licence,
// https://github.com/Smithay/smithay), for fern-comp.

use std::{ffi::OsString, sync::Arc};

use smithay::{
    backend::allocator::dmabuf::Dmabuf,
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{Seat, SeatState, keyboard::XkbConfig},
    reexports::{
        calloop::{
            EventLoop, Interest, LoopHandle, LoopSignal, Mode, PostAction, generic::Generic,
            ping::Ping,
        },
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        dmabuf::{DmabufGlobal, DmabufState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::XdgShellState,
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

use crate::LoopData;

pub struct Comp {
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,

    // Smithay State
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Comp>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,
    pub blur_state: crate::blur::BlurManagerState,
    pub dmabuf_state: DmabufState,
    /// The linux-dmabuf global, once a backend with a GPU created it.
    pub dmabuf_global: Option<DmabufGlobal>,
    /// Can the renderer use this client buffer? Set by the backend, which
    /// owns the renderer.
    pub dmabuf_import: Option<DmabufImport>,
    /// Explicit sync, when the GPU supports it (DRM backend).
    #[cfg(feature = "drm")]
    pub syncobj_state: Option<smithay::wayland::drm_syncobj::DrmSyncobjState>,
    /// For work that waits on file descriptors (GPU fences).
    pub loop_handle: LoopHandle<'static, LoopData>,
    /// Wakes the backend when something changed (set by backends that
    /// render on demand).
    pub redraw: Option<Ping>,

    pub seat: Seat<Self>,
    /// A request for the session owner (the DRM backend), set by input.
    pub session_request: Option<SessionRequest>,
    /// The window manager and every window's record (`crate::wm`).
    pub wm: crate::wm::WmHost,
}

/// Checks that the renderer can use a client's GPU buffer.
pub type DmabufImport = Box<dyn FnMut(&Dmabuf) -> bool>;

/// Things only the backend holding the session can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRequest {
    /// Switch to virtual terminal n.
    SwitchVt(i32),
    /// Leave the compositor (Ctrl+Alt+Backspace).
    Quit,
}

impl Comp {
    /// Sets up the protocols, the seat and the listening socket.
    ///
    /// # Errors
    /// No Wayland socket can be created (e.g. no `XDG_RUNTIME_DIR`), the
    /// default keymap does not compile, or the event loop refuses a source.
    pub fn new(
        event_loop: &mut EventLoop<'static, LoopData>,
        display: Display<Self>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();

        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let popups = PopupManager::default();
        let blur_state = crate::blur::BlurManagerState::new::<Self>(&dh);

        // A seat is a group of keyboards, pointer and touch devices.
        // A seat typically has a pointer and maintains a keyboard focus and a pointer focus.
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");

        // Notify clients that we have a keyboard, for the sake of the example we assume that keyboard is always present.
        // You may want to track keyboard hot-plug in real compositor.
        seat.add_keyboard(XkbConfig::default(), 200, 25)
            .map_err(|e| format!("keymap: {e:?}"))?;

        // Notify clients that we have a pointer (mouse)
        // Here we assume that there is always pointer plugged in
        seat.add_pointer();

        // A space represents a two-dimensional plane. Windows and Outputs can be mapped onto it.
        //
        // Windows get a position and stacking order through mapping.
        // Outputs become views of a part of the Space and can be rendered via Space::render_output.
        let space = Space::default();

        let socket_name = Self::init_wayland_listener(display, event_loop)?;

        // Get the loop signal, used to stop the event loop
        let loop_signal = event_loop.get_signal();

        Ok(Self {
            start_time,
            display_handle: dh,

            space,
            loop_signal,
            socket_name,

            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups,
            blur_state,
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            dmabuf_import: None,
            #[cfg(feature = "drm")]
            syncobj_state: None,
            loop_handle: event_loop.handle(),
            redraw: None,
            seat,
            session_request: None,
            wm: crate::wm::WmHost::default(),
        }
        .with_wm_global())
    }

    fn init_wayland_listener(
        display: Display<Comp>,
        event_loop: &mut EventLoop<LoopData>,
    ) -> Result<OsString, Box<dyn std::error::Error>> {
        // Creates a new listening socket, automatically choosing the next available `wayland` socket name.
        let listening_socket =
            ListeningSocketSource::new_auto().map_err(|e| format!("wayland socket: {e}"))?;

        // Get the name of the listening socket.
        // Clients will connect to this socket.
        let socket_name = listening_socket.socket_name().to_os_string();

        let loop_handle = event_loop.handle();

        loop_handle
            .insert_source(listening_socket, move |client_stream, (), state| {
                if let Err(e) = state
                    .display_handle
                    .insert_client(client_stream, Arc::new(ClientState::default()))
                {
                    tracing::warn!("rejected a client: {e}");
                }
            })
            .map_err(|e| format!("event loop: {}", e.error))?;

        // You also need to add the display itself to the event loop, so that client events will be processed by wayland-server.
        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // SAFETY: `get_mut` is unsafe because dropping the
                    // Display while the source holds it would be unsound;
                    // we only dispatch through it and never drop it here.
                    #[allow(unsafe_code)]
                    let dispatched =
                        unsafe { display.get_mut().dispatch_clients(&mut state.state) };
                    if let Err(e) = dispatched {
                        tracing::warn!("dispatching clients: {e}");
                    }
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| format!("event loop: {}", e.error))?;

        Ok(socket_name)
    }

    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.window_under(pos).and_then(|(window, location)| {
            window
                .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + location).to_f64()))
        })
    }
}

#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    /// The window manager the core started: the only client that may bind
    /// `fern_wm_v1`.
    pub is_wm: bool,
}

impl std::fmt::Debug for Comp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Comp")
            .field("socket_name", &self.socket_name)
            .finish_non_exhaustive()
    }
}

impl Comp {
    fn with_wm_global(mut self) -> Self {
        self.init_wm();
        self
    }

    /// Something on screen may have changed: render soon.
    pub fn request_redraw(&self) {
        if let Some(ping) = &self.redraw {
            ping.ping();
        }
    }

    /// After a frame was shown on `output`: frame callbacks to every
    /// window (so animating clients draw their next frame), then cleanup.
    pub fn frame_shown(&mut self, output: &smithay::output::Output) {
        let now = self.start_time.elapsed();
        for window in self.space.elements() {
            window.send_frame(output, now, Some(std::time::Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
        self.space.refresh();
        self.popups.cleanup();
    }

    /// Raises and focuses `window` (or clears focus), updating every
    /// window's activated state.
    pub fn focus(&mut self, window: Option<&Window>) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        if let Some(window) = window {
            self.space.raise_element(window, true);
        } else {
            for w in self.space.elements() {
                w.set_activated(false);
            }
        }
        let surface = window
            .and_then(|w| w.toplevel())
            .map(|t| t.wl_surface().clone());
        keyboard.set_focus(self, surface, serial);
        for w in self.space.elements() {
            if let Some(t) = w.toplevel() {
                t.send_pending_configure();
            }
        }
    }
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
