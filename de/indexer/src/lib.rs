//! fern-indexd (CLAUDE.md 6.8): the file part of Search. Every file and
//! folder name under the home folder, in SQLite with an FTS5 index over
//! the names, kept current with inotify, answered over D-Bus
//! (`org.fern.Search1`, docs/protocols/search.md).
//!
//! Names only for now: no file contents. Hidden files and folders (a
//! leading dot) are left out, and so is everything below them.

pub mod bus;
pub mod index;
pub mod watch;

pub use index::{Hit, Index};
