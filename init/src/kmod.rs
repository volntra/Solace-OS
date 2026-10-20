//! Kernel modules at boot (CLAUDE.md 6.1, early setup). Packages list the
//! modules they need in `/usr/lib/sol/modules.d/*.conf`; the administrator
//! adds or overrides files of the same name in `/etc/sol/modules.d/`. Each
//! line is a module name and its parameters, loaded in order:
//!
//! ```text
//! # NVIDIA: the core first, then what builds on it.
//! nvidia NVreg_PreserveVideoMemoryAllocations=1
//! nvidia-modeset
//! nvidia-drm modeset=1 fbdev=1
//! ```
//!
//! Modules are found by name under `/usr/lib/modules/<release>/`
//! (`-` and `_` are the same, `.ko` may be compressed: the kernel
//! decompresses it). There is no dependency resolution: a list names what
//! a module needs before it. A module that is already loaded or built in
//! is fine.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::log::LogStore;

/// Where module lists are read from, later directories overriding files
/// of the same name in earlier ones.
pub const CONF_DIRS: [&str; 2] = ["usr/lib/sol/modules.d", "etc/sol/modules.d"];

/// `finit_module` flag: the file is compressed, the kernel decompresses it.
const MODULE_INIT_COMPRESSED_FILE: i32 = 4;

/// One module to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub name: String,
    pub params: String,
}

/// Parses a module list: `name [param=value ...]` per line, `#` comments.
pub fn parse(text: &str) -> Vec<Request> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut words = l.split_whitespace();
            Request {
                name: words.next().unwrap_or("").to_string(),
                params: words.collect::<Vec<_>>().join(" "),
            }
        })
        .collect()
}

/// The module lists under `root`, by file name, `/etc` overriding `/usr`;
/// returned in file-name order.
pub fn requests(root: &Path) -> Vec<Request> {
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    for dir in CONF_DIRS {
        let Ok(rd) = fs::read_dir(root.join(dir)) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().is_some_and(|x| x == "conf") {
                files.insert(e.file_name().to_string_lossy().into_owned(), path);
            }
        }
    }
    files
        .values()
        .flat_map(|p| parse(&fs::read_to_string(p).unwrap_or_default()))
        .collect()
}

/// Module names compare with `-` and `_` treated alike.
pub fn normalize(name: &str) -> String {
    name.replace('-', "_")
}

/// Every module file under `dir`, by normalized name.
pub fn index(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                // The build tree (linux-build) is not where modules live.
                if p.file_name().is_none_or(|n| n != "build" && n != "source") {
                    todo.push(p);
                }
                continue;
            }
            let file = e.file_name().to_string_lossy().into_owned();
            for ext in [".ko", ".ko.zst", ".ko.xz", ".ko.gz"] {
                if let Some(stem) = file.strip_suffix(ext) {
                    out.insert(normalize(stem), p.clone());
                }
            }
        }
    }
    out
}

/// Loads one module file.
fn load(path: &Path, params: &str) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let params = CString::new(params).map_err(|_| "parameters contain a NUL byte".to_string())?;
    let compressed = path.extension().is_some_and(|e| e != "ko");
    let flags = if compressed {
        MODULE_INIT_COMPRESSED_FILE
    } else {
        0
    };
    match rustix::system::finit_module(&file, &params, flags) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => Ok(()),
        // The file is there: the kernel says a symbol the module uses is
        // missing, i.e. a module it needs is not loaded yet.
        Err(rustix::io::Errno::NOENT) => Err(format!(
            "{}: needs a module that is not loaded (list it first)",
            path.display()
        )),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Loads every listed module (PID 1, early setup). Problems are logged;
/// boot goes on without the module.
pub fn load_configured(log: &mut LogStore) {
    let wanted = requests(Path::new("/"));
    if wanted.is_empty() {
        return;
    }
    let release = rustix::system::uname()
        .release()
        .to_string_lossy()
        .into_owned();
    let dir = Path::new("/usr/lib/modules").join(&release);
    let modules = index(&dir);
    for r in wanted {
        let msg = match modules.get(&normalize(&r.name)) {
            Some(path) => match load(path, &r.params) {
                Ok(()) => format!("module {} loaded", r.name),
                Err(e) => format!("module {}: {e}", r.name),
            },
            None => format!("module {}: not found in {}", r.name, dir.display()),
        };
        log.push("sol-init", "init", &msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_parse_names_parameters_and_comments() {
        let r = parse("# NVIDIA\nnvidia NVreg_X=1  Y=2\n\n  nvidia-drm modeset=1 # inline\nzram\n");
        assert_eq!(
            r,
            vec![
                Request {
                    name: "nvidia".into(),
                    params: "NVreg_X=1 Y=2".into()
                },
                Request {
                    name: "nvidia-drm".into(),
                    params: "modeset=1".into()
                },
                Request {
                    name: "zram".into(),
                    params: String::new()
                },
            ]
        );
    }

    #[test]
    fn etc_overrides_usr_by_file_name_and_files_load_in_name_order() {
        let d = tempfile::tempdir().unwrap();
        let usr = d.path().join(CONF_DIRS[0]);
        let etc = d.path().join(CONF_DIRS[1]);
        fs::create_dir_all(&usr).unwrap();
        fs::create_dir_all(&etc).unwrap();
        fs::write(usr.join("20-nvidia.conf"), "nvidia\nnvidia-drm modeset=1\n").unwrap();
        fs::write(usr.join("10-zram.conf"), "zram\n").unwrap();
        fs::write(etc.join("20-nvidia.conf"), "nvidia\nnvidia-drm modeset=0\n").unwrap();
        fs::write(etc.join("README"), "not a list\n").unwrap();
        let names: Vec<String> = requests(d.path())
            .into_iter()
            .map(|r| format!("{} {}", r.name, r.params).trim().to_string())
            .collect();
        assert_eq!(names, ["zram", "nvidia", "nvidia-drm modeset=0"]);
    }

    #[test]
    fn the_index_finds_modules_by_either_spelling_and_skips_the_build_tree() {
        let d = tempfile::tempdir().unwrap();
        let k = d.path().join("kernel/drivers/block");
        fs::create_dir_all(&k).unwrap();
        fs::write(k.join("zram.ko.zst"), b"").unwrap();
        let v = d.path().join("extra/nvidia");
        fs::create_dir_all(&v).unwrap();
        fs::write(v.join("nvidia-drm.ko"), b"").unwrap();
        fs::create_dir_all(d.path().join("build/kernel")).unwrap();
        fs::write(d.path().join("build/kernel/zram.ko"), b"").unwrap();
        let idx = index(d.path());
        assert_eq!(idx.get("zram"), Some(&k.join("zram.ko.zst")));
        assert_eq!(
            idx.get(&normalize("nvidia-drm")),
            Some(&v.join("nvidia-drm.ko"))
        );
        assert_eq!(idx.len(), 2);
    }
}
