//! sol-comp: Solace's compositor core (CLAUDE.md 6.5). It owns the
//! screen, the Wayland protocols, input and rendering; window management
//! policy moves to the separate sol-wm process in Phase 5.

pub mod backdrop;
pub mod backend;
pub mod blur;
mod grabs;
mod handlers;
mod input;
pub mod placement;
pub mod render;
pub mod spring;
pub mod state;

use smithay::reexports::wayland_server::DisplayHandle;

pub use state::Comp;

/// What every event loop callback receives.
pub struct LoopData {
    pub state: Comp,
    pub display_handle: DisplayHandle,
}

impl std::fmt::Debug for LoopData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopData").finish_non_exhaustive()
    }
}
