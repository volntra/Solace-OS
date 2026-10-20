//! `org.fern.Search1` on the session bus (docs/protocols/search.md).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use zbus::{fdo, interface};

use crate::Index;

pub const NAME: &str = "org.fern.Search";
pub const PATH: &str = "/org/fern/Search";
/// The most results one query returns.
pub const MAX_RESULTS: u32 = 50;

/// The service object.
#[derive(Debug)]
pub struct Search {
    pub index: Arc<Mutex<Index>>,
    /// The first crawl is done.
    pub ready: Arc<AtomicBool>,
}

#[interface(name = "org.fern.Search1")]
impl Search {
    /// Files and folders whose names have words starting with every word
    /// of `text`, best first: (path, name, is a folder).
    fn query(&self, text: &str, limit: u32) -> fdo::Result<Vec<(String, String, bool)>> {
        let index = self.index.lock().unwrap_or_else(PoisonError::into_inner);
        index
            .search(text, limit.min(MAX_RESULTS))
            .map(|hits| {
                hits.into_iter()
                    .map(|h| (h.path, h.name, h.folder))
                    .collect()
            })
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// Whether the first crawl has finished (until then results may be
    /// missing).
    #[zbus(property)]
    fn ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }
}

/// Serves `search` on the session bus (or `address`) and takes the name.
///
/// # Errors
/// The bus cannot be reached, or the name is taken.
pub fn serve(address: Option<&str>, search: Search) -> zbus::Result<zbus::blocking::Connection> {
    let builder = match address {
        Some(a) => zbus::blocking::connection::Builder::address(a)?,
        None => zbus::blocking::connection::Builder::session()?,
    };
    builder.serve_at(PATH, search)?.name(NAME)?.build()
}
