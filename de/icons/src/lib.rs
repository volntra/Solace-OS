//! fern-icons: Fern's adaptive icon engine (CLAUDE.md 6.9,
//! docs/formats/icon.md). It renders `.fernicon` icons, and legacy
//! freedesktop icons, in five modes on the shared plate shape, and keeps
//! what it rendered in a disk cache. Qt reaches it through the C API in
//! [`capi`] (`include/fern-icons.h`).

mod color;
mod format;
mod render;
mod shape;

pub mod capi;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use resvg::tiny_skia::Pixmap;
use sha2::{Digest, Sha256};

pub use color::{FALLBACK as FALLBACK_ACCENT, tone, wallpaper_accent};
pub use format::Icon;
pub use render::Mode;

/// Part of every cache key: bump when rendering changes.
const ENGINE: &str = "fern-icons 1";

/// Icon sizes the engine draws, in pixels.
pub const MIN_SIZE: u32 = 8;
pub const MAX_SIZE: u32 = 1024;

/// Rendered icons kept in memory.
const MEMORY: usize = 512;

#[derive(Debug)]
pub enum Error {
    /// No icon with that id, and no legacy icon to fall back on.
    NotFound,
    /// An icon (or legacy file) that cannot be read or drawn.
    BadIcon(String),
    /// A size or mode the engine does not draw.
    BadRequest(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound => write!(f, "no such icon"),
            Error::BadIcon(e) => write!(f, "bad icon: {e}"),
            Error::BadRequest(e) => write!(f, "bad request: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// A rendered icon: `size` × `size` pixels, RGBA, premultiplied alpha.
#[derive(Debug)]
pub struct Image {
    pub size: u32,
    pub data: Vec<u8>,
}

/// What to draw.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// An icon id (`org.fern.Files`, `folder.documents`); may be empty.
    pub id: &'a str,
    /// A legacy icon file, used when `id` has no `.fernicon`.
    pub legacy: Option<&'a Path>,
    pub mode: Mode,
    /// `0xRRGGBB`; used by Tinted and Wallpaper.
    pub accent: u32,
    pub size: u32,
}

/// The engine: where icons are, and the caches.
#[derive(Debug)]
pub struct Engine {
    dirs: Vec<PathBuf>,
    cache: Option<PathBuf>,
    memory: Mutex<HashMap<[u8; 32], Arc<Image>>>,
}

/// An icon id is a file name: letters, digits, `.`, `-`, `_`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// What identifies a file's version: its name, size and change time.
fn stamp(hash: &mut Sha256, path: &Path) {
    hash.update(path.as_os_str().as_encoded_bytes());
    if let Ok(meta) = fs::metadata(path) {
        hash.update(meta.len().to_le_bytes());
        let t = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .unwrap_or_default();
        hash.update(t.as_nanos().to_le_bytes());
    }
    hash.update([0]);
}

impl Engine {
    /// An engine looking for icons in `dirs` (in order), caching rendered
    /// icons in `cache` (created when needed) if given.
    pub fn new(dirs: Vec<PathBuf>, cache: Option<PathBuf>) -> Self {
        Self {
            dirs,
            cache,
            memory: Mutex::new(HashMap::new()),
        }
    }

    /// The standard places: `$XDG_DATA_HOME/fern/icons`, each
    /// `$XDG_DATA_DIRS/fern/icons`, and the cache in
    /// `$XDG_CACHE_HOME/fern/icons` (or under `~/.cache`). For development,
    /// `$FERN_ICONS_DIRS` (colon-separated) replaces the places, without a
    /// disk cache.
    pub fn standard() -> Self {
        if let Some(list) = std::env::var_os("FERN_ICONS_DIRS").filter(|v| !v.is_empty()) {
            let dirs = std::env::split_paths(&list)
                .filter(|d| !d.as_os_str().is_empty())
                .collect();
            return Self::new(dirs, None);
        }
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = env("HOME");
        let mut dirs = Vec::new();
        if let Some(d) =
            env("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local/share")))
        {
            dirs.push(d.join("fern/icons"));
        }
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        for d in data_dirs.split(':').filter(|d| !d.is_empty()) {
            dirs.push(Path::new(d).join("fern/icons"));
        }
        let cache = env("XDG_CACHE_HOME")
            .or_else(|| home.map(|h| h.join(".cache")))
            .map(|c| c.join("fern/icons"));
        Self::new(dirs, cache)
    }

    /// Where the icon `id` is, if anywhere.
    pub fn find(&self, id: &str) -> Option<PathBuf> {
        if !valid_id(id) {
            return None;
        }
        self.dirs
            .iter()
            .map(|d| d.join(format!("{id}.fernicon")))
            .find(|d| d.join("icon.toml").is_file())
    }

    /// Draws an icon (from the memory or disk cache when it can).
    ///
    /// # Errors
    /// [`Error::NotFound`] when there is neither a `.fernicon` nor a legacy
    /// file, [`Error::BadIcon`] when it cannot be read or drawn,
    /// [`Error::BadRequest`] for a size outside the range.
    pub fn render(&self, req: &Request<'_>) -> Result<Arc<Image>, Error> {
        if !(MIN_SIZE..=MAX_SIZE).contains(&req.size) {
            return Err(Error::BadRequest(format!(
                "size {} (from {MIN_SIZE} to {MAX_SIZE})",
                req.size
            )));
        }
        let native = self.find(req.id);
        let source: &Path = match (&native, req.legacy) {
            (Some(dir), _) => dir,
            (None, Some(file)) => file,
            (None, None) => return Err(Error::NotFound),
        };

        let mut hash = Sha256::new();
        hash.update(ENGINE);
        hash.update([0]);
        if let Some(dir) = &native {
            let mut files: Vec<PathBuf> = fs::read_dir(dir)
                .map_err(|e| Error::BadIcon(format!("{}: {e}", dir.display())))?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .collect();
            files.sort();
            for f in &files {
                stamp(&mut hash, f);
            }
        } else {
            stamp(&mut hash, source);
        }
        hash.update(req.mode.name());
        let accent = if req.mode.uses_accent() {
            req.accent & 0x00FF_FFFF
        } else {
            0
        };
        hash.update(accent.to_le_bytes());
        hash.update(req.size.to_le_bytes());
        let key: [u8; 32] = hash.finalize().into();

        if let Some(hit) = self
            .memory
            .lock()
            .map_err(|_| Error::BadRequest("poisoned".into()))?
            .get(&key)
        {
            return Ok(hit.clone());
        }
        let file = self
            .cache
            .as_ref()
            .map(|c| c.join(format!("{}.png", hex(&key))));
        let cached = file
            .as_ref()
            .and_then(|f| Pixmap::load_png(f).ok())
            .filter(|p| p.width() == req.size && p.height() == req.size);
        let pixmap = if let Some(p) = cached {
            p
        } else {
            {
                let p = if let Some(dir) = &native {
                    render::native(&Icon::load(dir)?, req.mode, accent, req.size)?
                } else {
                    let image = render::load_legacy(source, req.size)?;
                    render::legacy(&image, req.mode, accent, req.size)
                };
                if let Some(f) = &file {
                    // A failed write only costs a render next time.
                    let _ = write_atomic(f, &p);
                }
                p
            }
        };
        let image = Arc::new(Image {
            size: req.size,
            data: pixmap.take(),
        });
        let mut memory = self
            .memory
            .lock()
            .map_err(|_| Error::BadRequest("poisoned".into()))?;
        if memory.len() >= MEMORY {
            memory.clear();
        }
        memory.insert(key, image.clone());
        Ok(image)
    }

    /// Forgets the icons kept in memory (the disk cache stays).
    pub fn forget(&self) {
        if let Ok(mut m) = self.memory.lock() {
            m.clear();
        }
    }
}

/// The freedesktop icon file of the app `app_id`: its desktop entry's
/// `Icon=` (an absolute path, or a name looked up in the hicolor theme:
/// the scalable SVG, else the largest PNG), searching
/// `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`.
pub fn legacy_icon(app_id: &str) -> Option<PathBuf> {
    if !valid_id(app_id) {
        return None;
    }
    let mut data = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        data.push(PathBuf::from(home));
    } else if let Some(home) = std::env::var_os("HOME") {
        data.push(PathBuf::from(home).join(".local/share"));
    }
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    data.extend(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    legacy_icon_in(app_id, &data)
}

/// hicolor sizes to look in, best first.
const SIZES: [&str; 7] = [
    "scalable", "512x512", "256x256", "128x128", "96x96", "64x64", "48x48",
];

fn legacy_icon_in(app_id: &str, data: &[PathBuf]) -> Option<PathBuf> {
    let entry = data
        .iter()
        .map(|d| d.join("applications").join(format!("{app_id}.desktop")))
        .find(|p| p.is_file())?;
    let text = fs::read_to_string(entry).ok()?;
    let mut in_entry = false;
    let mut icon = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(v) = line.strip_prefix("Icon=") {
            icon = Some(v.trim().to_string());
        }
    }
    let icon = icon.filter(|i| !i.is_empty())?;
    if Path::new(&icon).is_absolute() {
        return Path::new(&icon).is_file().then(|| PathBuf::from(icon));
    }
    if !valid_id(&icon) {
        return None;
    }
    for d in data {
        for size in SIZES {
            for ext in ["svg", "png"] {
                let p = d.join(format!("icons/hicolor/{size}/apps/{icon}.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn hex(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Writes a PNG beside its place, then renames it there.
fn write_atomic(path: &Path, pixmap: &Pixmap) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let png = pixmap.encode_png().map_err(std::io::Error::other)?;
    let tmp = dir.join(format!(
        ".{}.tmp{}",
        path.file_name().unwrap_or_default().display(),
        std::process::id()
    ));
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&png)?;
    drop(f);
    fs::rename(&tmp, path)
}

#[cfg(test)]
// Colours read as #rrggbb; pixel tests name their channels.
#[allow(clippy::unreadable_literal, clippy::many_single_char_names)]
mod tests {
    use super::*;

    fn write_icon(dir: &Path, id: &str, key: &str) {
        let icon = dir.join(format!("{id}.fernicon"));
        fs::create_dir_all(&icon).unwrap();
        fs::write(
            icon.join("background.svg"),
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><rect width="1024" height="1024" fill="{key}"/></svg>"#),
        )
        .unwrap();
        fs::write(
            icon.join("foreground.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><circle data-role="primary" cx="512" cy="512" r="200" fill="#fff"/></svg>"##,
        )
        .unwrap();
        fs::write(
            icon.join("icon.toml"),
            format!("format = 1\nkey = \"{key}\"\n[roles]\nprimary = \"#FFFFFF\"\n"),
        )
        .unwrap();
    }

    fn req(id: &str, mode: Mode, accent: u32) -> Request<'_> {
        Request {
            id,
            legacy: None,
            mode,
            accent,
            size: 64,
        }
    }

    #[test]
    fn ids_are_file_names() {
        assert!(valid_id("org.fern.Files"));
        assert!(valid_id("folder.documents"));
        assert!(!valid_id("../etc/passwd"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(".hidden"));
        assert!(!valid_id(""));
    }

    #[test]
    fn finds_icons_in_order_and_falls_back_to_legacy() {
        let user = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        write_icon(system.path(), "org.example.App", "#2D64C8");
        let engine = Engine::new(vec![user.path().into(), system.path().into()], None);
        assert!(
            engine
                .find("org.example.App")
                .unwrap()
                .starts_with(system.path())
        );
        // The user's copy wins.
        write_icon(user.path(), "org.example.App", "#B4430F");
        assert!(
            engine
                .find("org.example.App")
                .unwrap()
                .starts_with(user.path())
        );
        assert!(engine.find("../org.example.App").is_none());

        assert!(matches!(
            engine.render(&req("org.example.Missing", Mode::Default, 0)),
            Err(Error::NotFound)
        ));
        // A legacy PNG when there is no .fernicon.
        let png = system.path().join("legacy.png");
        let mut p = Pixmap::new(32, 32).unwrap();
        p.fill(resvg::tiny_skia::Color::from_rgba8(200, 30, 30, 255));
        p.save_png(&png).unwrap();
        let r = Request {
            legacy: Some(&png),
            ..req("org.example.Missing", Mode::Default, 0)
        };
        let image = engine.render(&r).unwrap();
        assert_eq!(image.data.len(), 64 * 64 * 4);
        assert!(matches!(
            engine.render(&Request { size: 4, ..r }),
            Err(Error::BadRequest(_))
        ));
    }

    #[test]
    fn caches_on_disk_and_notices_changes() {
        let icons = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        write_icon(icons.path(), "org.example.App", "#2D64C8");
        let engine = Engine::new(vec![icons.path().into()], Some(cache.path().join("icons")));
        let first = engine
            .render(&req("org.example.App", Mode::Tinted, 0xB4430F))
            .unwrap();
        let files = || fs::read_dir(cache.path().join("icons")).unwrap().count();
        assert_eq!(files(), 1);
        // The same from a new engine: read back from disk, same pixels.
        let again = Engine::new(vec![icons.path().into()], Some(cache.path().join("icons")));
        assert_eq!(
            again
                .render(&req("org.example.App", Mode::Tinted, 0xB4430F))
                .unwrap()
                .data,
            first.data
        );
        assert_eq!(files(), 1);
        // The accent only matters in the accent modes.
        let old = engine
            .render(&req("org.example.App", Mode::Default, 1))
            .unwrap();
        engine
            .render(&req("org.example.App", Mode::Default, 2))
            .unwrap();
        assert_eq!(files(), 2);
        engine
            .render(&req("org.example.App", Mode::Tinted, 0x2D64C8))
            .unwrap();
        assert_eq!(files(), 3);
        // An edited icon is drawn again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_icon(icons.path(), "org.example.App", "#62CC8C");
        engine.forget();
        let changed = engine
            .render(&req("org.example.App", Mode::Default, 0))
            .unwrap();
        assert_eq!(files(), 4);
        assert_ne!(changed.data, old.data);
    }

    #[test]
    fn finds_an_apps_own_icon_through_its_desktop_entry() {
        let data = tempfile::tempdir().unwrap();
        let d = data.path();
        fs::create_dir_all(d.join("applications")).unwrap();
        fs::create_dir_all(d.join("icons/hicolor/48x48/apps")).unwrap();
        fs::create_dir_all(d.join("icons/hicolor/256x256/apps")).unwrap();
        fs::write(d.join("icons/hicolor/48x48/apps/org.example.Big.png"), b"x").unwrap();
        fs::write(
            d.join("icons/hicolor/256x256/apps/org.example.Big.png"),
            b"x",
        )
        .unwrap();
        fs::write(
            d.join("applications/org.example.App.desktop"),
            "[Desktop Entry]\nName=App\nIcon=org.example.Big\n[Desktop Action new]\nIcon=other\n",
        )
        .unwrap();
        let dirs = vec![d.to_path_buf()];
        // The largest size wins.
        assert_eq!(
            legacy_icon_in("org.example.App", &dirs).unwrap(),
            d.join("icons/hicolor/256x256/apps/org.example.Big.png")
        );
        assert!(legacy_icon_in("org.example.Missing", &dirs).is_none());
        // Names that would escape the theme are refused.
        fs::write(
            d.join("applications/org.example.Bad.desktop"),
            "[Desktop Entry]\nIcon=../../etc/x\n",
        )
        .unwrap();
        assert!(legacy_icon_in("org.example.Bad", &dirs).is_none());
        assert!(legacy_icon_in("../applications/x", &dirs).is_none());
    }

    #[test]
    fn a_broken_icon_is_an_error_not_a_crash() {
        let icons = tempfile::tempdir().unwrap();
        write_icon(icons.path(), "org.example.App", "#2D64C8");
        fs::write(
            icons.path().join("org.example.App.fernicon/foreground.svg"),
            "<svg",
        )
        .unwrap();
        let engine = Engine::new(vec![icons.path().into()], None);
        assert!(matches!(
            engine.render(&req("org.example.App", Mode::Default, 0)),
            Err(Error::BadIcon(_))
        ));
    }
}
