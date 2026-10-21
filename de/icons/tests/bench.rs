//! The re-tint target (CLAUDE.md 6.9): with a warm cache, a full dock
//! changes mode in under 16 ms. A dock here is 12 icons at 128 px (48 pt
//! icons on a 2x display, the size `AppIcon` asks for), drawn one after
//! another by a fresh engine whose memory cache is empty, so every icon
//! comes from the disk cache. Timing is only asserted in release builds
//! (`just icons-bench`); debug builds just run it.

use std::path::PathBuf;
use std::time::Instant;

use fernicons::{Engine, Mode, Request};

const DOCK: [&str; 12] = [
    "org.fern.Files",
    "org.fern.Settings",
    "org.fern.Terminal",
    "org.fern.Installer",
    "fern.app",
    "folder.documents",
    "folder.pictures",
    "folder.music",
    "file.text",
    "drive.external",
    "folder.downloads",
    "trash",
];

#[test]
fn a_dock_retints_within_a_frame() {
    let set = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("set/icons");
    let cache = tempfile::tempdir().unwrap();
    let modes = [
        Mode::Default,
        Mode::Dark,
        Mode::Tinted,
        Mode::Wallpaper,
        Mode::Clear,
    ];
    let draw = |engine: &Engine, mode: Mode| {
        for id in DOCK {
            engine
                .render(&Request {
                    id,
                    legacy: None,
                    mode,
                    accent: 0x00B4_430F,
                    size: 128,
                })
                .unwrap();
        }
    };
    // Cold: draws every icon in every mode, filling the disk cache.
    let cold = Engine::new(vec![set.clone()], Some(cache.path().into()));
    let start = Instant::now();
    for mode in modes {
        draw(&cold, mode);
    }
    let cold_ms = start.elapsed().as_secs_f64() * 1000.0 / 5.0;

    // Warm: a new engine (as after a restart), every mode from disk.
    let warm = Engine::new(vec![set], Some(cache.path().into()));
    let mut worst = 0.0f64;
    for mode in modes {
        let start = Instant::now();
        draw(&warm, mode);
        worst = worst.max(start.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "dock of {}: cold {cold_ms:.1} ms a mode, warm worst {worst:.2} ms a mode",
        DOCK.len()
    );
    if !cfg!(debug_assertions) {
        assert!(
            worst < 16.0,
            "re-tinting the dock took {worst:.2} ms (target 16)"
        );
    }
}
