//! Where fern-comp draws: `winit` (nested in another desktop, for
//! development), `drm` (the real display: KMS, libinput, libseat) and
//! `headless` (offscreen, for tests).

#[cfg(feature = "drm")]
pub mod drm;
pub mod headless;
#[cfg(feature = "nested")]
pub mod winit;

/// The desktop background until the wallpaper is drawn: the forest
/// palette's deepest tone (#0F2018).
pub const BACKGROUND: [f32; 4] = [0.059, 0.125, 0.094, 1.0];
