// Adapted from Smithay's smallvil example (v0.7.0, MIT licence,
// https://github.com/Smithay/smithay), for fern-comp.

use crate::{
    grabs::resize_grab,
    state::{ClientState, Comp},
};
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_shm,
    reexports::wayland_server::{
        Client,
        protocol::{wl_buffer, wl_surface::WlSurface},
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent,
            is_sync_subsurface,
        },
        shm::{ShmHandler, ShmState},
    },
};

impl CompositorHandler for Comp {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        super::wait_for_gpu_before_commit(surface);
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for(&root) {
                window.on_commit();
                self.wm_window_committed(&window);
            }
        }

        self.layer_commit(surface);
        self.xdg_commit(surface);
        resize_grab::handle_commit(&mut self.space, surface);
        self.request_redraw();
    }
}

impl BufferHandler for Comp {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Comp {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

delegate_compositor!(Comp);
delegate_shm!(Comp);
