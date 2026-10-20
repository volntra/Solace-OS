// Adapted from Smithay's smallvil example (v0.7.0, MIT licence,
// https://github.com/Smithay/smithay), for fern-comp.

use smithay::{
    delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{PopupKind, Window, find_popup_root_surface, get_popup_toplevel_coords},
    input::{
        Seat,
        pointer::{Focus, GrabStartData as PointerGrabStartData},
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Resource,
            protocol::{wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Rectangle, Serial},
    wayland::{
        compositor::with_states,
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            XdgToplevelSurfaceData,
        },
    },
};

use crate::{
    Comp,
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab},
};
use fern_wm_proto::server::fern_wm_v1::RequestKind;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;

impl XdgShellHandler for Comp {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // Mapped into the space when it is placed (by the window manager,
        // or the core's fallback once it has a size).
        self.wm_new_window(&Window::new_wayland_window(surface));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.wm_window_closed(&window);
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.wm_names_changed(&window);
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.wm_names_changed(&window);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        self.forward_request(&surface, RequestKind::Maximize);
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        self.forward_request(&surface, RequestKind::Unmaximize);
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        self.forward_request(&surface, RequestKind::Fullscreen);
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        self.forward_request(&surface, RequestKind::Unfullscreen);
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.wm_client_request(&window, RequestKind::Minimize, 0);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::from_resource(&seat) else {
            return;
        };

        let wl_surface = surface.wl_surface();

        if let Some(start_data) = check_grab(&seat, wl_surface, serial) {
            let Some(pointer) = seat.get_pointer() else {
                return;
            };

            let Some(window) = self.window_for(wl_surface) else {
                return;
            };
            let Some(initial_window_location) = self.space.element_location(&window) else {
                return;
            };

            let grab = MoveSurfaceGrab {
                start_data,
                window,
                initial_window_location,
            };

            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let Some(seat) = Seat::from_resource(&seat) else {
            return;
        };

        let wl_surface = surface.wl_surface();

        if let Some(start_data) = check_grab(&seat, wl_surface, serial) {
            let Some(pointer) = seat.get_pointer() else {
                return;
            };

            let Some(window) = self.window_for(wl_surface) else {
                return;
            };
            let Some(initial_window_location) = self.space.element_location(&window) else {
                return;
            };
            let initial_window_size = window.geometry().size;

            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Resizing);
            });

            surface.send_pending_configure();

            let grab = ResizeSurfaceGrab::start(
                start_data,
                window,
                edges.into(),
                Rectangle::new(initial_window_location, initial_window_size),
            );

            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // TODO popup grabs
    }
}

// Xdg Shell
delegate_xdg_shell!(Comp);

// Server-side decoration: we always draw our own titlebar when a client
// lets us (whatever mode it prefers), so every window looks the same.
impl XdgDecorationHandler for Comp {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        self.use_server_side(&toplevel);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        self.use_server_side(&toplevel);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        self.use_server_side(&toplevel);
    }
}

delegate_xdg_decoration!(Comp);

fn check_grab(
    seat: &Seat<Comp>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<PointerGrabStartData<Comp>> {
    let pointer = seat.get_pointer()?;

    // Check that this surface has a click grab.
    if !pointer.has_grab(serial) {
        return None;
    }

    let start_data = pointer.grab_start_data()?;

    let (focus, _) = start_data.focus.as_ref()?;
    // If the focus was for a different surface, ignore the request.
    if !focus.id().same_client_as(&surface.id()) {
        return None;
    }

    Some(start_data)
}

impl Comp {
    /// The window whose toplevel is `surface`, mapped or not.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.wm
            .windows
            .values()
            .find(|r| {
                r.window
                    .toplevel()
                    .is_some_and(|t| t.wl_surface() == surface)
            })
            .map(|r| r.window.clone())
    }

    fn use_server_side(&mut self, toplevel: &ToplevelSurface) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        if let Some(window) = self.window_for(toplevel.wl_surface()) {
            self.wm_server_side(&window);
        }
    }

    fn forward_request(&mut self, surface: &ToplevelSurface, kind: RequestKind) {
        let handled = self
            .window_for(surface.wl_surface())
            .is_some_and(|w| self.wm_client_request(&w, kind, 0));
        // The protocol wants a configure in reply, even a refusal.
        if !handled && surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    /// Initial configures, and popups (on every commit).
    pub fn xdg_commit(&mut self, surface: &WlSurface) {
        if let Some(window) = self.window_for(surface)
            && let Some(toplevel) = window.toplevel()
        {
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|d| d.lock().ok())
                    .is_some_and(|d| d.initial_configure_sent)
            });
            if !initial_configure_sent {
                toplevel.send_configure();
            }
        }

        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(ref xdg)) = self.popups.find_popup(surface)
            && !xdg.is_initial_configure_sent()
        {
            // The initial configure is always allowed.
            let _ = xdg.send_configure();
        }
    }
}

impl Comp {
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self
            .space
            .elements()
            .find(|w| w.toplevel().unwrap().wl_surface() == &root)
        else {
            return;
        };

        let output = self.space.outputs().next().unwrap();
        let output_geo = self.space.output_geometry(output).unwrap();
        let window_geo = self.space.element_geometry(window).unwrap();

        // The target geometry for the positioner should be relative to its parent's geometry, so
        // we will compute that here.
        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;

        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}
