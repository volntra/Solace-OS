// Adapted from Smithay's smallvil example (v0.7.0, MIT licence,
// https://github.com/Smithay/smithay), for sol-comp.

mod compositor;
mod xdg_shell;

use crate::state::Comp;

//
// Wl Seat
//

use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
    set_data_device_focus,
};
use smithay::{delegate_data_device, delegate_output, delegate_seat};

impl SeatHandler for Comp {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Comp> {
        &mut self.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

delegate_seat!(Comp);

//
// Wl Data Device
//

impl SelectionHandler for Comp {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Comp {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Comp {}
impl ServerDndGrabHandler for Comp {}

delegate_data_device!(Comp);

//
// Wl Output & Xdg Output
//

impl OutputHandler for Comp {}
delegate_output!(Comp);
