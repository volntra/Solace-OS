//! GPU buffers from clients (linux-dmabuf) and explicit synchronisation
//! (linux-drm-syncobj, which NVIDIA's driver relies on). A buffer is
//! accepted only if our renderer can import it; a commit with a GPU buffer
//! waits until the client's rendering into it has finished, on its
//! explicit acquire point when it gives one, on the buffer's implicit
//! fence otherwise.

use smithay::{
    backend::allocator::dmabuf::Dmabuf,
    delegate_dmabuf,
    reexports::{
        calloop::Interest,
        wayland_server::{Client, Resource, protocol::wl_surface::WlSurface},
    },
    wayland::{
        compositor::{
            BufferAssignment, SurfaceAttributes, add_blocker, add_pre_commit_hook, with_states,
        },
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier, get_dmabuf},
    },
};

use crate::state::{ClientState, Comp};

impl DmabufHandler for Comp {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        if self
            .dmabuf_import
            .as_mut()
            .is_some_and(|import| import(&dmabuf))
        {
            let _ = notifier.successful::<Comp>();
        } else {
            notifier.failed();
        }
    }
}
delegate_dmabuf!(Comp);

#[cfg(feature = "drm")]
impl smithay::wayland::drm_syncobj::DrmSyncobjHandler for Comp {
    fn drm_syncobj_state(&mut self) -> Option<&mut smithay::wayland::drm_syncobj::DrmSyncobjState> {
        self.syncobj_state.as_mut()
    }
}
#[cfg(feature = "drm")]
smithay::delegate_drm_syncobj!(Comp);

/// Makes `surface`'s commits with GPU buffers wait for the GPU.
pub fn wait_for_gpu_before_commit(surface: &WlSurface) {
    add_pre_commit_hook::<Comp, _>(surface, |state, _dh, surface| {
        #[cfg(feature = "drm")]
        let mut acquire = None;
        let dmabuf = with_states(surface, |states| {
            #[cfg(feature = "drm")]
            acquire.clone_from(
                &states
                    .cached_state
                    .get::<smithay::wayland::drm_syncobj::DrmSyncobjCachedState>()
                    .pending()
                    .acquire_point,
            );
            match states
                .cached_state
                .get::<SurfaceAttributes>()
                .pending()
                .buffer
                .as_ref()
            {
                Some(BufferAssignment::NewBuffer(buffer)) => get_dmabuf(buffer).cloned().ok(),
                _ => None,
            }
        });
        let (Some(dmabuf), Some(client)) = (dmabuf, surface.client()) else {
            return;
        };
        #[cfg(feature = "drm")]
        if let Some(point) = acquire
            && let Ok((blocker, source)) = point.generate_blocker()
        {
            let client = client.clone();
            if state
                .loop_handle
                .insert_source(source, move |(), &mut (), data| {
                    unblock(&client, data);
                    Ok(())
                })
                .is_ok()
            {
                add_blocker(surface, blocker);
                return;
            }
        }
        if let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ)
            && state
                .loop_handle
                .insert_source(source, move |(), _, data| {
                    unblock(&client, data);
                    Ok(())
                })
                .is_ok()
        {
            add_blocker(surface, blocker);
        }
    });
}

/// The GPU finished: let the client's blocked commits through.
fn unblock(client: &Client, data: &mut crate::LoopData) {
    if let Some(c) = client.get_data::<ClientState>() {
        let dh = data.display_handle.clone();
        c.compositor_state.blocker_cleared(&mut data.state, &dh);
    }
}
