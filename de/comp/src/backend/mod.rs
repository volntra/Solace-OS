//! Where sol-comp draws: `winit` (nested in another desktop, for
//! development), `drm` (the real display: KMS, libinput, libseat) and
//! `headless` (offscreen, for tests).

#[cfg(feature = "drm")]
pub mod drm;
pub mod headless;
#[cfg(feature = "nested")]
pub mod winit;

/// The desktop background until the wallpaper is drawn: the dusk palette's
/// deepest tone.
pub const BACKGROUND: [f32; 4] = [0.075, 0.07, 0.14, 1.0];
