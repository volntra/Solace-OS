//! fern-indexd: indexes file and folder names under the home folder and
//! answers Search over D-Bus (docs/protocols/search.md).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use fernindex::bus::{self, Search};
use fernindex::{Index, watch::Watcher};

const USAGE: &str = "usage: fern-indexd [--root DIR] [--db FILE] [--address ADDR]
  --root     what to index (default: the home folder)
  --db       the index (default: ~/.cache/fern/index.sqlite)
  --address  serve on this bus address (default: the session bus)";

fn main() -> ExitCode {
    let mut root = std::env::var_os("HOME").map(PathBuf::from);
    let mut db = None;
    let mut address = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--db" => db = args.next().map(PathBuf::from),
            "--address" => address = args.next(),
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(root) = root else {
        eprintln!("fern-indexd: no --root and no HOME");
        return ExitCode::FAILURE;
    };
    let db = db.unwrap_or_else(|| {
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map_or_else(|| root.join(".cache"), PathBuf::from)
            .join("fern/index.sqlite")
    });
    if let Some(dir) = db.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("fern-indexd: {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    let index = match Index::open(&db) {
        Ok(i) => Arc::new(Mutex::new(i)),
        Err(e) => {
            // A cache: start again from nothing rather than not at all.
            eprintln!("fern-indexd: {}: {e}; starting a new index", db.display());
            let _ = std::fs::remove_file(&db);
            match Index::open(&db) {
                Ok(i) => Arc::new(Mutex::new(i)),
                Err(e) => {
                    eprintln!("fern-indexd: {}: {e}", db.display());
                    return ExitCode::FAILURE;
                }
            }
        }
    };
    let ready = Arc::new(AtomicBool::new(false));
    let search = Search {
        index: index.clone(),
        ready: ready.clone(),
    };
    // Searches are answered from the start, from what the last run left.
    let _conn = match bus::serve(address.as_deref(), search) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fern-indexd: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut watcher = match Watcher::new(&root, index) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("fern-indexd: inotify: {e}");
            return ExitCode::FAILURE;
        }
    };
    let started = std::time::Instant::now();
    if let Err(e) = watcher.full_crawl() {
        eprintln!("fern-indexd: {e}");
        return ExitCode::FAILURE;
    }
    ready.store(true, Ordering::Relaxed);
    eprintln!(
        "fern-indexd: {} indexed in {:.1} s",
        root.display(),
        started.elapsed().as_secs_f64()
    );
    watcher.run()
}
