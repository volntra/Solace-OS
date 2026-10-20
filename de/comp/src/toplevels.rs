//! wlr-foreign-toplevel-management (CLAUDE.md 6.5): the window list for
//! the dock and the app switcher. They see every window's title, app id
//! and state, can activate, minimize, maximize, fullscreen and close it,
//! and say where its dock icon is (`set_rectangle`), which minimize
//! animates towards.
//!
//! Only trusted clients see the global: the parts of the desktop the core
//! starts itself (the window manager, the shell). An app could otherwise
//! close or minimize every other app's windows.
//!
//! What the window manager decides (minimize, maximize, fullscreen,
//! activate) goes to it as a `window_request`, exactly like the window's
//! own request would.

use std::collections::HashMap;

use smithay::{
    reexports::wayland_server::{
        Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
        backend::ClientId, protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point},
};
use wayland_protocols_wlr::foreign_toplevel::v1::server::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use crate::state::{ClientState, Comp};
use fern_wm_proto::server::fern_wm_v1::RequestKind;

/// The version we offer (3 adds fullscreen and parent).
const VERSION: u32 = 3;

/// The protocol's `minimized` state bit (not an xdg state; the window
/// manager sets it with `configure`).
pub const MINIMIZED: u32 = 256;

/// Every bound manager and the handles made for each window.
#[derive(Debug, Default)]
pub struct Toplevels {
    managers: Vec<ZwlrForeignToplevelManagerV1>,
    handles: HashMap<u32, Vec<ZwlrForeignToplevelHandleV1>>,
}

impl Comp {
    /// Offers the protocol (to trusted clients only).
    pub fn init_toplevels(&mut self) {
        self.display_handle
            .create_global::<Comp, ZwlrForeignToplevelManagerV1, ()>(VERSION, ());
    }

    /// A window is on screen for the first time: every manager hears of
    /// it.
    pub fn toplevel_new(&mut self, id: u32) {
        for manager in self.toplevels.managers.clone() {
            self.toplevel_handle(&manager, id);
        }
    }

    fn toplevel_handle(&mut self, manager: &ZwlrForeignToplevelManagerV1, id: u32) {
        let Some(client) = manager.client() else {
            return;
        };
        let Ok(handle) = client.create_resource::<ZwlrForeignToplevelHandleV1, u32, Comp>(
            &self.display_handle,
            manager.version(),
            id,
        ) else {
            return;
        };
        manager.toplevel(&handle);
        self.toplevels
            .handles
            .entry(id)
            .or_default()
            .push(handle.clone());
        self.toplevel_describe(&handle, id);
    }

    /// Sends one handle everything about window `id`, then `done`.
    fn toplevel_describe(&self, handle: &ZwlrForeignToplevelHandleV1, id: u32) {
        let Some(rec) = self.wm.windows.get(&id) else {
            return;
        };
        let (app_id, title) = crate::wm::names(&rec.window);
        handle.title(title);
        handle.app_id(app_id);
        if let Some(client) = handle.client() {
            let frame = rec.window.geometry();
            let at = self.space.element_location(&rec.window).unwrap_or_default();
            for output in self.space.outputs() {
                let on = self
                    .space
                    .output_geometry(output)
                    .is_some_and(|g| g.overlaps(smithay::utils::Rectangle::new(at, frame.size)));
                if on {
                    for o in output.client_outputs(&client) {
                        handle.output_enter(&o);
                    }
                }
            }
        }
        handle.state(state_array(rec.states));
        handle.done();
    }

    /// Window `id` changed (title, app id, state): every handle hears it.
    pub fn toplevel_changed(&mut self, id: u32) {
        let Some(handles) = self.toplevels.handles.get(&id) else {
            return;
        };
        let Some(rec) = self.wm.windows.get(&id) else {
            return;
        };
        let (app_id, title) = crate::wm::names(&rec.window);
        for h in handles {
            h.title(title.clone());
            h.app_id(app_id.clone());
            h.state(state_array(rec.states));
            h.done();
        }
    }

    /// Window `id` is gone.
    pub fn toplevel_closed(&mut self, id: u32) {
        for h in self.toplevels.handles.remove(&id).unwrap_or_default() {
            h.closed();
        }
    }

    /// Where a surface's top-left is in the desktop, if it is a window's.
    fn surface_origin(&self, surface: &WlSurface) -> Option<Point<i32, Logical>> {
        let window = self
            .space
            .elements()
            .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))?;
        Some(self.space.element_location(window)? - window.geometry().loc)
    }

    fn toplevel_request(&mut self, id: u32, kind: RequestKind) {
        let Some(window) = self.wm.windows.get(&id).map(|r| r.window.clone()) else {
            return;
        };
        self.wm_client_request(&window, kind, 0);
    }
}

/// The protocol's state array from our state bits.
fn state_array(states: u32) -> Vec<u8> {
    use zwlr_foreign_toplevel_handle_v1::State;
    let mut out = Vec::new();
    for (bit, st) in [
        (2, State::Maximized),
        (MINIMIZED, State::Minimized),
        (1, State::Activated),
        (4, State::Fullscreen),
    ] {
        if states & bit != 0 {
            out.extend_from_slice(&(st as u32).to_ne_bytes());
        }
    }
    out
}

impl GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> for Comp {
    fn bind(
        state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ());
        state.toplevels.managers.push(manager.clone());
        let mapped: Vec<u32> = state
            .wm
            .windows
            .iter()
            .filter(|(_, r)| r.size.is_some())
            .map(|(id, _)| *id)
            .collect();
        for id in mapped {
            state.toplevel_handle(&manager, id);
        }
    }

    fn can_view(client: Client, _global_data: &()) -> bool {
        client.get_data::<ClientState>().is_some_and(|d| d.trusted)
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Comp {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            state.toplevels.managers.retain(|m| m != resource);
            resource.finished();
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        resource: &ZwlrForeignToplevelManagerV1,
        _data: &(),
    ) {
        state.toplevels.managers.retain(|m| m != resource);
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, u32> for Comp {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        id: &u32,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Request as R;
        let id = *id;
        match request {
            R::SetMaximized => state.toplevel_request(id, RequestKind::Maximize),
            R::UnsetMaximized => state.toplevel_request(id, RequestKind::Unmaximize),
            R::SetMinimized => state.toplevel_request(id, RequestKind::Minimize),
            R::UnsetMinimized | R::Activate { .. } => {
                state.toplevel_request(id, RequestKind::Activate);
            }
            R::SetFullscreen { .. } => state.toplevel_request(id, RequestKind::Fullscreen),
            R::UnsetFullscreen => state.toplevel_request(id, RequestKind::Unfullscreen),
            R::Close => {
                if let Some(t) = state.wm.windows.get(&id).and_then(|r| r.window.toplevel()) {
                    t.send_close();
                }
            }
            R::SetRectangle {
                surface,
                x,
                y,
                width,
                height,
            } => {
                let rect = if width > 0 && height > 0 {
                    state
                        .surface_origin(&surface)
                        .or_else(|| state.layer_origin(&surface))
                        .map(|o| (o.x + x, o.y + y, width, height))
                } else {
                    Some((0, 0, 0, 0))
                };
                if let Some(r) = rect {
                    state.wm_icon(id, r);
                } else {
                    tracing::debug!("set_rectangle on a surface that is not a window");
                }
            }
            R::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        resource: &ZwlrForeignToplevelHandleV1,
        id: &u32,
    ) {
        if let Some(list) = state.toplevels.handles.get_mut(id) {
            list.retain(|h| h != resource);
        }
    }
}
