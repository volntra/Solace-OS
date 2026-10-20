//! sol-init: Solace's init and service manager (PID 1), solctl, and
//! sol-log. Unit format: docs/formats/unit.md.
//!
//! The decision-making parts ([`unit`], [`graph`], [`policy`],
//! [`manager`], [`proto`], [`log`]) are pure and unit-tested; [`runtime`]
//! carries out their decisions with real processes and sockets, and
//! [`pid1`] holds what only PID 1 does (mounts, root assembly, reboot).

pub mod graph;
pub mod kmod;
pub mod log;
pub mod manager;
pub mod pid1;
pub mod policy;
pub mod proto;
pub mod runtime;
pub mod unit;

use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    Unit(String),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unit(m) | Error::Io(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn io(path: &Path, e: &std::io::Error) -> Self {
        Error::Io(format!("{}: {e}", path.display()))
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
