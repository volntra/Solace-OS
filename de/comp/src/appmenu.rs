//! The global menu (CLAUDE.md 6.8): apps say where their menus are with
//! KDE's `org_kde_kwin_appmenu` (a D-Bus service and object path exporting
//! `com.canonical.dbusmenu`), and the shell hears the active window's with
//! `fern_shell_v1.app_menu`.
//!
//! Any client may name an address for its own surfaces only; it is the
//! app's claim, and the shell treats it as untrusted.

use std::collections::HashMap;

use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
    backend::{ClientId, ObjectId},
};
use wayland_protocols_plasma::appmenu::server::{
    org_kde_kwin_appmenu::{self, OrgKdeKwinAppmenu},
    org_kde_kwin_appmenu_manager::{self, OrgKdeKwinAppmenuManager},
};

use crate::state::Comp;

/// The version we offer (2 adds the manager's `release`).
const VERSION: u32 = 2;

/// Every surface's menu address, and what the shell last heard.
#[derive(Debug, Default)]
pub struct AppMenus {
    /// surface -> (service, path).
    addresses: HashMap<ObjectId, (String, String)>,
    /// appmenu object -> its surface.
    objects: HashMap<ObjectId, ObjectId>,
    sent: Option<(String, String)>,
}

impl Comp {
    /// Offers the protocol to every client.
    pub fn init_appmenu(&mut self) {
        self.display_handle
            .create_global::<Comp, OrgKdeKwinAppmenuManager, ()>(VERSION, ());
    }

    /// The active window's menu address (empty strings when none).
    pub fn active_app_menu(&self) -> (String, String) {
        self.wm
            .windows
            .values()
            .find(|r| r.states & 1 != 0)
            .and_then(|r| r.window.toplevel())
            .and_then(|t| self.appmenus.addresses.get(&t.wl_surface().id()))
            .cloned()
            .unwrap_or_default()
    }

    /// Tells the shell if the active window's menus moved.
    pub fn app_menu_update(&mut self) {
        let now = self.active_app_menu();
        if self.appmenus.sent.as_ref() == Some(&now) {
            return;
        }
        self.shell_app_menu(&now.0, &now.1);
        self.appmenus.sent = Some(now);
    }
}

impl GlobalDispatch<OrgKdeKwinAppmenuManager, ()> for Comp {
    fn bind(
        _state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<OrgKdeKwinAppmenuManager>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<OrgKdeKwinAppmenuManager, ()> for Comp {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &OrgKdeKwinAppmenuManager,
        request: org_kde_kwin_appmenu_manager::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let org_kde_kwin_appmenu_manager::Request::Create { id, surface } = request {
            let menu = data_init.init(id, ());
            state.appmenus.objects.insert(menu.id(), surface.id());
        }
    }
}

impl Dispatch<OrgKdeKwinAppmenu, ()> for Comp {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &OrgKdeKwinAppmenu,
        request: org_kde_kwin_appmenu::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let org_kde_kwin_appmenu::Request::SetAddress {
            service_name,
            object_path,
        } = request
            && let Some(surface) = state.appmenus.objects.get(&resource.id())
        {
            // Bounded: these go on to the shell and to D-Bus.
            let ok = service_name.len() <= 255 && object_path.len() <= 1024;
            if ok && !service_name.is_empty() && object_path.starts_with('/') {
                state
                    .appmenus
                    .addresses
                    .insert(surface.clone(), (service_name, object_path));
            } else {
                state.appmenus.addresses.remove(surface);
            }
            state.app_menu_update();
        }
    }

    fn destroyed(state: &mut Self, _client: ClientId, resource: &OrgKdeKwinAppmenu, _data: &()) {
        // "If not applicable, clients should remove this object."
        if let Some(surface) = state.appmenus.objects.remove(&resource.id()) {
            state.appmenus.addresses.remove(&surface);
            state.app_menu_update();
        }
    }
}
