// Adapted from Smithay's smallvil example (v0.7.0, MIT licence,
// https://github.com/Smithay/smithay), for sol-comp.

use std::{ffi::OsString, sync::Arc};

use smithay::{
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{Seat, SeatState, keyboard::XkbConfig},
    reexports::{
        calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
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

    pub seat: Seat<Self>,
}

impl Comp {
    /// Sets up the protocols, the seat and the listening socket.
    ///
    /// # Errors
    /// No Wayland socket can be created (e.g. no `XDG_RUNTIME_DIR`), the
    /// default keymap does not compile, or the event loop refuses a source.
    pub fn new(
        event_loop: &mut EventLoop<LoopData>,
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
            seat,
        })
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
        self.space
            .element_under(pos)
            .and_then(|(window, location)| {
                window
                    .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + location).to_f64()))
            })
    }
}

#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl std::fmt::Debug for Comp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Comp")
            .field("socket_name", &self.socket_name)
            .finish_non_exhaustive()
    }
}

/// Marks a window that has been placed (on its first sized commit).
struct Placed;

impl Comp {
    /// Places a new window once its size is known (its first commit with
    /// a buffer), using [`crate::placement`].
    pub fn place_new_window(&mut self, window: &Window) {
        if window.user_data().get::<Placed>().is_some() {
            return;
        }
        let size = window.geometry().size;
        if size.w <= 0 || size.h <= 0 {
            return;
        }
        window.user_data().insert_if_missing(|| Placed);
        let Some(area) = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
        else {
            return;
        };
        let taken: Vec<(i32, i32)> = self
            .space
            .elements()
            .filter(|w| *w != window)
            .filter_map(|w| self.space.element_location(w))
            .map(|p| (p.x, p.y))
            .collect();
        let (x, y) = crate::placement::place(
            (area.loc.x, area.loc.y, area.size.w, area.size.h),
            (size.w, size.h),
            &taken,
        );
        self.space.map_element(window.clone(), (x, y), true);
    }
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
