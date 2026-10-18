//! libspk: the Solace package manager library.
//!
//! Formats are specified in `docs/formats/` (recipe, package, repo).

pub mod error;
pub mod hash;
pub mod manifest;
pub mod package;
pub mod recipe;
pub mod sign;
pub mod version;

pub use error::{Error, Result};
