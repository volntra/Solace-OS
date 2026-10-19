//! libspk: the Solace package manager library.
//!
//! Formats are specified in `docs/formats/` (recipe, package, repo).

pub mod bootstrap;
pub mod btrfs;
pub mod build;
pub mod db;
pub mod error;
pub mod fetch;
pub mod hash;
pub mod manifest;
pub mod package;
pub mod recipe;
pub mod repo;
pub mod sign;
pub mod solve;
pub mod system;
pub mod tx;
pub mod version;

pub use error::{Error, Result};
