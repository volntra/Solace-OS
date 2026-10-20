//! fern-comp: Fern's compositor core (CLAUDE.md 6.5). It owns the
//! screen, the Wayland protocols, input and rendering; window management
//! policy lives in the separate fern-wm process, the menu bar and dock in
//! the shell (layer surfaces, `layers`).

pub mod appmenu;
pub mod backdrop;
pub mod backend;
pub mod blur;
pub mod cursor;
mod grabs;
mod handlers;
mod input;
pub mod layers;
pub mod nightshift;
pub mod placement;
pub mod render;
pub mod screenshot;
pub mod spring;
pub mod state;
pub mod stats;
pub mod text;
pub mod toplevels;
pub mod wm;

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
