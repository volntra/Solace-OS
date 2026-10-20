//! Crawling and watching: a full crawl at start (forgetting what went
//! away while we were not running), then inotify keeps the index current.
//! inotify, not fanotify: fanotify's directory events need
//! `CAP_SYS_ADMIN`, and this runs as the user.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};

use crate::Index;

/// Stops adding entries past this many (a home folder with millions of
/// files still gets a working, if partial, index).
pub const MAX_ENTRIES: u64 = 2_000_000;

/// The index's writer: crawls and follows changes under one root.
#[derive(Debug)]
pub struct Watcher {
    inotify: Inotify,
    dirs: HashMap<WatchDescriptor, PathBuf>,
    root: PathBuf,
    index: Arc<Mutex<Index>>,
    /// This crawl's mark (see `Index::sweep`).
    seen: i64,
    full: bool,
    out_of_watches: bool,
}

/// Hidden entries (and everything below them) are not indexed.
fn hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

impl Watcher {
    /// # Errors
    /// inotify is not available.
    pub fn new(root: &Path, index: Arc<Mutex<Index>>) -> io::Result<Self> {
        Ok(Watcher {
            inotify: Inotify::init()?,
            dirs: HashMap::new(),
            root: root.to_path_buf(),
            index,
            seen: 0,
            full: false,
            out_of_watches: false,
        })
    }

    fn index(&self) -> std::sync::MutexGuard<'_, Index> {
        self.index.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Crawls the whole root, then forgets entries it did not find.
    ///
    /// # Errors
    /// The index refused a write.
    pub fn full_crawl(&mut self) -> rusqlite::Result<()> {
        self.seen = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| {
                i64::try_from(d.as_nanos() / 1000).unwrap_or(i64::MAX)
            });
        self.full = false;
        let root = self.root.clone();
        self.watch(&root);
        self.crawl(&root)?;
        if let Some(r) = root.to_str() {
            self.index().sweep(r, self.seen)?;
        }
        Ok(())
    }

    fn watch(&mut self, dir: &Path) {
        if self.out_of_watches {
            return;
        }
        let mask = WatchMask::CREATE
            | WatchMask::DELETE
            | WatchMask::MOVED_FROM
            | WatchMask::MOVED_TO
            | WatchMask::DONT_FOLLOW
            | WatchMask::ONLYDIR;
        match self.inotify.watches().add(dir, mask) {
            // Adding an inode watched already returns its descriptor: a
            // folder that moved is now known by its new path.
            Ok(wd) => {
                self.dirs.insert(wd, dir.to_path_buf());
            }
            Err(e) if e.raw_os_error() == Some(28) => {
                // ENOSPC: fs.inotify.max_user_watches reached.
                eprintln!(
                    "fern-indexd: out of inotify watches; later changes below {} are not followed",
                    dir.display()
                );
                self.out_of_watches = true;
            }
            Err(_) => {}
        }
    }

    /// Puts everything below `dir` (not `dir` itself) and watches every
    /// folder there. Each folder is one transaction, so searches go on
    /// meanwhile.
    fn crawl(&mut self, dir: &Path) -> rusqlite::Result<()> {
        let mut pending = vec![dir.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut found: Vec<(String, bool)> = Vec::new();
            for e in entries.flatten() {
                if hidden(&e.file_name()) {
                    continue;
                }
                let path = e.path();
                // Not followed: a link is an entry, never a way in.
                let folder = e.file_type().is_ok_and(|t| t.is_dir());
                if folder {
                    pending.push(path.clone());
                }
                if let Some(p) = path.to_str() {
                    found.push((p.to_owned(), folder));
                }
            }
            self.watch(&dir);
            if self.full {
                continue;
            }
            let seen = self.seen;
            let full = {
                let mut index = self.index.lock().unwrap_or_else(PoisonError::into_inner);
                index.batch(|i| {
                    for (p, folder) in &found {
                        i.put(p, *folder, seen)?;
                    }
                    i.len()
                })?
            } >= MAX_ENTRIES;
            if full {
                eprintln!("fern-indexd: {MAX_ENTRIES} entries indexed; the rest is left out");
                self.full = true;
            }
        }
        Ok(())
    }

    /// Waits for changes and applies them.
    ///
    /// # Errors
    /// Reading inotify failed, or the index refused a write.
    pub fn handle(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut buf = [0u8; 16384];
        let events: Vec<(WatchDescriptor, EventMask, Option<std::ffi::OsString>)> = self
            .inotify
            .read_events_blocking(&mut buf)?
            .map(|e| (e.wd, e.mask, e.name.map(OsStr::to_os_string)))
            .collect();
        for (wd, mask, name) in events {
            if mask.contains(EventMask::Q_OVERFLOW) {
                // Events were lost: look again.
                self.full_crawl()?;
                continue;
            }
            if mask.contains(EventMask::IGNORED) {
                self.dirs.remove(&wd);
                continue;
            }
            let (Some(dir), Some(name)) = (self.dirs.get(&wd).cloned(), name) else {
                continue;
            };
            if hidden(&name) {
                continue;
            }
            let path = dir.join(&name);
            let Some(p) = path.to_str() else {
                continue;
            };
            if mask.intersects(EventMask::DELETE | EventMask::MOVED_FROM) {
                self.index().remove(p)?;
            } else if mask.intersects(EventMask::CREATE | EventMask::MOVED_TO) {
                let folder = mask.contains(EventMask::ISDIR);
                if !self.full {
                    self.index().put(p, folder, self.seen)?;
                }
                if folder {
                    // Moved in with everything inside, or made and filled
                    // before we got here.
                    self.watch(&path);
                    self.crawl(&path)?;
                }
            }
        }
        Ok(())
    }

    /// Follows changes for ever.
    pub fn run(mut self) -> ! {
        loop {
            if let Err(e) = self.handle() {
                eprintln!("fern-indexd: {e}");
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }
}
