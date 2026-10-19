//! Where sol-comp draws: `winit` (nested in another desktop, for
//! development), `headless` (offscreen, for tests); DRM/KMS on real
//! hardware next (Phase 4, stage 5).

pub mod headless;
pub mod winit;
