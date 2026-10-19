//! Background blur requests: KDE's `org_kde_kwin_blur` protocol, which Qt
//! and KDE apps already speak and our own shell uses for its materials.
//!
//! A client creates a blur object for a surface, sets a region (none
//! means the whole surface) and commits it; like all surface state it
//! takes effect on the surface's next commit.

use std::sync::Mutex;

use smithay::{
    reexports::wayland_server::{
        Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
        backend::GlobalId,
        protocol::{wl_region::WlRegion, wl_surface::WlSurface},
    },
    utils::{Logical, Rectangle, Size},
    wayland::compositor::{
        Cacheable, RectangleKind, RegionAttributes, get_region_attributes, with_states,
    },
};
use wayland_protocols_plasma::blur::server::{
    org_kde_kwin_blur::{self, OrgKdeKwinBlur},
    org_kde_kwin_blur_manager::{self, OrgKdeKwinBlurManager},
};

/// What a surface asked to have blurred behind it.
#[derive(Clone, Debug, Default)]
pub enum Blur {
    #[default]
    Off,
    /// The whole surface.
    Whole,
    /// Part of it, in surface coordinates.
    Region(RegionAttributes),
}

impl Blur {
    /// The blurred rectangles of a surface of `size`, surface-local.
    pub fn rects(&self, size: Size<i32, Logical>) -> Vec<Rectangle<i32, Logical>> {
        let bounds = Rectangle::from_size(size);
        match self {
            Blur::Off => Vec::new(),
            Blur::Whole => vec![bounds],
            Blur::Region(region) => {
                let mut rects: Vec<Rectangle<i32, Logical>> = Vec::new();
                for (kind, rect) in &region.rects {
                    match kind {
                        RectangleKind::Add => rects.extend(rect.intersection(bounds)),
                        RectangleKind::Subtract => {
                            rects = rects
                                .into_iter()
                                .flat_map(|r| r.subtract_rect(*rect))
                                .collect();
                        }
                    }
                }
                rects.retain(|r| !r.is_empty());
                rects
            }
        }
    }
}

/// Double-buffered per-surface blur state.
#[derive(Clone, Debug, Default)]
pub struct BlurState(pub Blur);

impl Cacheable for BlurState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }
    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// The blur currently in effect on `surface`.
pub fn surface_blur(surface: &WlSurface) -> Blur {
    with_states(surface, |states| {
        states.cached_state.get::<BlurState>().current().0.clone()
    })
}

fn set_pending(surface: &WlSurface, blur: Blur) {
    with_states(surface, |states| {
        states.cached_state.get::<BlurState>().pending().0 = blur;
    });
}

/// The blur manager global.
#[derive(Debug)]
pub struct BlurManagerState {
    global: GlobalId,
}

impl BlurManagerState {
    pub fn new<D>(dh: &DisplayHandle) -> Self
    where
        D: GlobalDispatch<OrgKdeKwinBlurManager, ()> + 'static,
    {
        BlurManagerState {
            global: dh.create_global::<D, OrgKdeKwinBlurManager, ()>(1, ()),
        }
    }

    pub fn global(&self) -> GlobalId {
        self.global.clone()
    }
}

/// A blur object: its surface and the region set since its last commit.
#[derive(Debug)]
pub struct BlurObject {
    surface: WlSurface,
    region: Mutex<Blur>,
}

impl<D> GlobalDispatch<OrgKdeKwinBlurManager, (), D> for BlurManagerState
where
    D: Dispatch<OrgKdeKwinBlurManager, ()> + 'static,
{
    fn bind(
        _state: &mut D,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<OrgKdeKwinBlurManager>,
        _global_data: &(),
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(resource, ());
    }
}

impl<D> Dispatch<OrgKdeKwinBlurManager, (), D> for BlurManagerState
where
    D: Dispatch<OrgKdeKwinBlur, BlurObject> + 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &OrgKdeKwinBlurManager,
        request: org_kde_kwin_blur_manager::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            org_kde_kwin_blur_manager::Request::Create { id, surface } => {
                data_init.init(
                    id,
                    BlurObject {
                        surface,
                        // A blur object without a region blurs everything.
                        region: Mutex::new(Blur::Whole),
                    },
                );
            }
            org_kde_kwin_blur_manager::Request::Unset { surface } => {
                set_pending(&surface, Blur::Off);
            }
            _ => {}
        }
    }
}

impl<D> Dispatch<OrgKdeKwinBlur, BlurObject, D> for BlurManagerState {
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &OrgKdeKwinBlur,
        request: org_kde_kwin_blur::Request,
        data: &BlurObject,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        let Ok(mut region) = data.region.lock() else {
            return;
        };
        match request {
            org_kde_kwin_blur::Request::SetRegion { region: r } => {
                *region = r.as_ref().map_or(Blur::Whole, region_blur);
            }
            org_kde_kwin_blur::Request::Commit if data.surface.is_alive() => {
                set_pending(&data.surface, region.clone());
            }
            // Releasing the object keeps the blur, as in KWin; a commit
            // for a destroyed surface has nothing to apply to.
            _ => {}
        }
    }
}

fn region_blur(region: &WlRegion) -> Blur {
    Blur::Region(get_region_attributes(region))
}

/// Implements the blur protocol for a compositor state type.
#[macro_export]
macro_rules! delegate_blur {
    ($ty:ty) => {
        smithay::reexports::wayland_server::delegate_global_dispatch!($ty: [
            wayland_protocols_plasma::blur::server::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager: ()
        ] => $crate::blur::BlurManagerState);
        smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            wayland_protocols_plasma::blur::server::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager: ()
        ] => $crate::blur::BlurManagerState);
        smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            wayland_protocols_plasma::blur::server::org_kde_kwin_blur::OrgKdeKwinBlur: $crate::blur::BlurObject
        ] => $crate::blur::BlurManagerState);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn off_and_whole() {
        let size = Size::from((300, 200));
        assert!(Blur::Off.rects(size).is_empty());
        assert_eq!(Blur::Whole.rects(size), vec![rect(0, 0, 300, 200)]);
    }

    #[test]
    fn regions_are_clipped_to_the_surface_and_subtract() {
        let size = Size::from((300, 200));
        let region = RegionAttributes {
            rects: vec![
                (RectangleKind::Add, rect(-10, -10, 120, 60)),
                (RectangleKind::Add, rect(250, 150, 100, 100)),
                (RectangleKind::Subtract, rect(0, 0, 300, 20)),
            ],
        };
        let mut got = Blur::Region(region).rects(size);
        got.sort_by_key(|r| (r.loc.x, r.loc.y));
        assert_eq!(got, vec![rect(0, 20, 110, 30), rect(250, 150, 50, 50)]);
    }
}
