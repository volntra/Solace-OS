//! Every icon in the shipped set (de/icons/set/icons) loads and renders
//! in every mode, and the set has what the desktop asks for by name.

use std::path::PathBuf;

use fernicons::{Engine, Mode, Request};

fn set() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("set/icons")
}

#[test]
fn every_icon_renders_in_every_mode() {
    let engine = Engine::new(vec![set()], None);
    let mut ids: Vec<String> = std::fs::read_dir(set())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".fernicon").map(str::to_string))
        .collect();
    ids.sort();
    assert!(ids.len() >= 40, "only {} icons", ids.len());
    for id in &ids {
        for mode in Mode::ALL {
            let image = engine
                .render(&Request {
                    id,
                    legacy: None,
                    mode,
                    accent: 0x002D_64C8,
                    size: 48,
                })
                .unwrap_or_else(|e| panic!("{id} in {}: {e}", mode.name()));
            // Something is drawn, and the corners stay clear.
            assert!(image.data.chunks(4).any(|p| p[3] > 200), "{id} {}: empty", mode.name());
            assert_eq!(image.data[3], 0, "{id} {}: a corner is painted", mode.name());
        }
    }
}

#[test]
fn the_set_has_the_icons_the_desktop_names() {
    let engine = Engine::new(vec![set()], None);
    for id in [
        "fern.app",
        "org.fern.Files",
        "org.fern.Settings",
        "org.fern.Terminal",
        "org.fern.Installer",
        "folder",
        "folder.downloads",
        "folder.documents",
        "trash",
        "trash.full",
        "drive.internal",
        "drive.external",
        "file",
        "file.text",
        "file.image",
    ] {
        assert!(engine.find(id).is_some(), "no icon {id}");
    }
}
