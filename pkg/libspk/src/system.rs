//! System snapshots and transactions on a running Solace
//! (docs/formats/system.md): every change to the system is made in a new
//! btrfs snapshot of the booted one, which is sealed and added to the boot
//! menu as the default; the running system is never touched.

use std::fs::{self, File};
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::mount::{UnmountFlags, mount_bind, unmount};
use serde::{Deserialize, Serialize};

use crate::btrfs;
use crate::error::{Error, IoContext, Result};
use crate::sign::Keyring;
use crate::solve::{Plan, Request};
use crate::tx::{self, Event, Root, Sources};

/// Paths relative to the live root (normally `/`).
pub const BTRFS_TOP: &str = "run/sol/btrfs";
pub const BOOTED: &str = "run/sol/booted";
const LOCK: &str = "run/sol/spk.lock";
const STAGE: &str = "run/sol/tx";
const BOOT_SETTINGS: &str = "etc/sol/boot.toml";
const ESP: &str = "boot";
/// Under the btrfs top level.
const SNAPSHOTS: &str = "@snapshots";
/// Paths inside a system (its `/usr`).
pub const META: &str = "lib/sol/system.toml";
const KERNEL: &str = "lib/sol/boot/vmlinuz";
const INITRAMFS: &str = "lib/sol/boot/initramfs.img";
const SOL_INIT: &str = "lib/sol/sol-init";
const OS_RELEASE: &str = "lib/os-release";
const TX_LOCK: &str = "lib/spk/lock";

/// `lib/sol/system.toml` of a system.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub id: String,
    pub version: String,
    pub created: u64,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub summary: String,
}

/// One system on the volume.
#[derive(Clone, Debug)]
pub struct SystemInfo {
    pub id: u64,
    /// Read-only: complete and bootable. Unsealed means interrupted.
    pub sealed: bool,
    pub meta: Option<Meta>,
}

/// What a finished transaction did.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub id: u64,
    pub summary: String,
    /// Systems deleted because the boot menu no longer keeps them.
    pub pruned: Vec<u64>,
    /// Problems after the new system was already the default (it stays).
    pub warnings: Vec<String>,
}

/// A running Solace system, as seen by spk.
#[derive(Clone, Debug)]
pub struct Live {
    root: PathBuf,
    top: PathBuf,
    /// The booted system's subvolume, e.g. `@snapshots/2`.
    booted: String,
    root_uuid: String,
    esp_uuid: String,
}

impl Live {
    /// The running Solace rooted at `root` (normally `/`), or `None` when
    /// `root` is not one (a scratch root, a build host).
    ///
    /// # Errors
    /// A Solace whose system volume or kernel command line is incomplete.
    pub fn detect(root: &Path) -> Result<Option<Live>> {
        let booted_file = root.join(BOOTED);
        if !booted_file.exists() {
            return Ok(None);
        }
        let booted = fs::read_to_string(&booted_file)
            .at(&booted_file)?
            .trim()
            .to_string();
        let top = root.join(BTRFS_TOP);
        if !btrfs::is_subvolume(&top) {
            return Err(Error::Conflict(format!(
                "the system volume is not mounted at {}; system updates are unavailable",
                top.display()
            )));
        }
        let cmdline = fs::read_to_string(root.join("proc/cmdline")).unwrap_or_default();
        let (root_uuid, esp_uuid) = boot_ids(&cmdline)?;
        Ok(Some(Live {
            root: root.to_path_buf(),
            top,
            booted,
            root_uuid,
            esp_uuid,
        }))
    }

    /// The booted system's id.
    pub fn booted_id(&self) -> Option<u64> {
        subvol_id(&self.booted)
    }

    fn snapshots(&self) -> PathBuf {
        self.top.join(SNAPSHOTS)
    }

    fn system_dir(&self, id: u64) -> PathBuf {
        self.snapshots().join(id.to_string())
    }

    /// Every system on the volume, by id.
    ///
    /// # Errors
    /// I/O errors and malformed metadata of a sealed system.
    pub fn list(&self) -> Result<Vec<SystemInfo>> {
        let dir = self.snapshots();
        let mut out = Vec::new();
        for e in fs::read_dir(&dir).at(&dir)? {
            let e = e.at(&dir)?;
            let Some(id) = e.file_name().to_str().and_then(|n| n.parse::<u64>().ok()) else {
                continue;
            };
            if id == 0 || !btrfs::is_subvolume(&e.path()) {
                continue;
            }
            let sealed = btrfs::is_read_only(&e.path())?;
            let meta_path = e.path().join(META);
            let meta = if sealed && meta_path.exists() {
                let text = fs::read_to_string(&meta_path).at(&meta_path)?;
                Some(
                    toml::from_str(&text)
                        .map_err(|err| Error::invalid(format!("{}: {err}", meta_path.display())))?,
                )
            } else {
                None
            };
            out.push(SystemInfo { id, sealed, meta });
        }
        out.sort_by_key(|s| s.id);
        Ok(out)
    }

    fn lock(&self) -> Result<File> {
        let p = self.root.join(LOCK);
        let f = File::create(&p).at(&p)?;
        f.try_lock()
            .map_err(|_| Error::Conflict("another system transaction is running".into()))?;
        Ok(f)
    }

    fn settings(&self) -> Result<(bootgen::Settings, String)> {
        let p = self.root.join(BOOT_SETTINGS);
        if !p.exists() {
            return Ok((bootgen::Settings::default(), String::new()));
        }
        let text = fs::read_to_string(&p).at(&p)?;
        let s =
            toml::from_str(&text).map_err(|e| Error::invalid(format!("{}: {e}", p.display())))?;
        Ok((s, text))
    }

    fn save_default(&self, raw: &str, default: Option<&str>) -> Result<()> {
        let p = self.root.join(BOOT_SETTINGS);
        write_atomic(&p, &with_default(raw, default)?)
    }

    /// The boot menu's inputs: every sealed system, these settings.
    fn params(
        &self,
        systems: &[SystemInfo],
        settings: bootgen::Settings,
    ) -> Result<bootgen::Params> {
        let mut out = Vec::new();
        for s in systems.iter().filter(|s| s.sealed) {
            let m = s
                .meta
                .as_ref()
                .ok_or_else(|| Error::invalid(format!("system {} has no {META}", s.id)))?;
            let dir = self.system_dir(s.id);
            out.push(bootgen::System {
                id: m.id.clone(),
                version: m.version.clone(),
                created: m.created,
                subvol: format!("{SNAPSHOTS}/{}", s.id),
                kernel: dir.join(KERNEL),
                initramfs: dir.join(INITRAMFS),
            });
        }
        let os_name = fs::read_to_string(self.root.join("usr").join(OS_RELEASE))
            .ok()
            .and_then(|t| os_release_value(&t, "NAME"))
            .unwrap_or_else(|| "Solace".into());
        Ok(bootgen::Params {
            esp_uuid: self.esp_uuid.clone(),
            root_uuid: self.root_uuid.clone(),
            systems: out,
            other_os: Vec::new(),
            settings,
            os_name,
        })
    }

    fn write_boot_menu(&self, params: &bootgen::Params) -> Result<()> {
        let esp = self.root.join(ESP);
        if !esp.join("grub").is_dir() {
            return Err(Error::Conflict(format!(
                "the ESP is not mounted at {}",
                esp.display()
            )));
        }
        bootgen::apply(&esp, params, &bootgen::free_bytes)
            .map(|_| ())
            .map_err(|e| Error::Conflict(format!("boot menu: {e}")))
    }

    /// Makes `req` a new system and the boot menu's default (see the module
    /// docs). `Ok(None)`: nothing to change.
    ///
    /// # Errors
    /// Any failure before the boot menu is written; the new snapshot is
    /// then deleted and the running system and boot menu are unchanged.
    pub fn transact(
        &self,
        req: &Request,
        sources: &Sources,
        on: &mut dyn FnMut(Event),
    ) -> Result<Option<Outcome>> {
        let _lock = self.lock()?;
        let snaps = self.snapshots();
        let booted_id = self.booted_id().ok_or_else(|| {
            Error::invalid(format!(
                "booted from {}, not a system snapshot",
                self.booted
            ))
        })?;
        for s in self
            .list()?
            .iter()
            .filter(|s| !s.sealed && s.id != booted_id)
        {
            btrfs::delete(&snaps, &s.id.to_string())?;
        }
        let id = next_id(self.list()?.iter().map(|s| s.id));
        btrfs::snapshot(&self.top.join(&self.booted), &snaps, &id.to_string(), false)?;
        let stage = self.root.join(STAGE).join(id.to_string());
        let built = self.build_system(id, booted_id, &stage, req, sources, on);
        let _ = unmount(stage.join("usr"), UnmountFlags::DETACH);
        let _ = fs::remove_dir_all(&stage);
        let summary = match built {
            Ok(Some(summary)) => summary,
            other => {
                btrfs::delete(&snaps, &id.to_string())?;
                return other.map(|_| None);
            }
        };

        test_pause("before-seal", id);
        // 7-8: seal, then the boot menu with the new system as default.
        let installed = (|| {
            btrfs::sync_fs(&self.system_dir(id))?;
            btrfs::seal(&self.system_dir(id))?;
            let (mut settings, _) = self.settings()?;
            settings.default = Some(id.to_string());
            let params = self.params(&self.list()?, settings)?;
            self.write_boot_menu(&params)?;
            Ok::<_, Error>(params)
        })();
        let params = match installed {
            Ok(p) => p,
            Err(e) => {
                btrfs::delete(&snaps, &id.to_string())?;
                return Err(e);
            }
        };

        // 9: the new system boots from now on; tidy up. It is in the boot
        // menu already, so problems here are reported, not undone.
        let mut warnings = Vec::new();
        match self.settings() {
            Ok((_, raw)) => {
                if let Err(e) = self.save_default(&raw, None) {
                    warnings.push(format!("boot settings: {e}"));
                }
            }
            Err(e) => warnings.push(format!("boot settings: {e}")),
        }
        let kept: Vec<u64> = params
            .kept()
            .iter()
            .filter_map(|s| s.id.parse().ok())
            .collect();
        let sealed: Vec<u64> = self
            .list()?
            .iter()
            .filter(|s| s.sealed)
            .map(|s| s.id)
            .collect();
        let mut pruned = Vec::new();
        for old in prune_list(&sealed, &kept, booted_id) {
            match btrfs::delete(&snaps, &old.to_string()) {
                Ok(()) => pruned.push(old),
                Err(e) => warnings.push(format!("could not delete system {old}: {e}")),
            }
        }
        Ok(Some(Outcome {
            id,
            summary,
            pruned,
            warnings,
        }))
    }

    /// Steps 4-6 in the new (still writable) system: the package
    /// transaction, the initramfs trigger, the metadata. `Ok(None)`:
    /// nothing to change.
    fn build_system(
        &self,
        id: u64,
        parent: u64,
        stage: &Path,
        req: &Request,
        sources: &Sources,
        on: &mut dyn FnMut(Event),
    ) -> Result<Option<String>> {
        let new = self.system_dir(id);
        let usr = stage.join("usr");
        fs::create_dir_all(&usr).at(&usr)?;
        mount_bind(&new, &usr).map_err(|e| Error::Io {
            path: usr.clone(),
            source: e.into(),
        })?;
        let keyring = Keyring::load(&self.root)?;
        let mut planned = (Vec::new(), Vec::new());
        let plan: Plan = {
            let mut root = Root::open_with_keyring(stage, keyring)?;
            tx::run(&mut root, req, sources, &mut |ev| {
                if let Event::Planned { install, remove } = &ev {
                    planned = (install.clone(), remove.clone());
                }
                on(ev);
            })?
        };
        let _ = fs::remove_file(new.join(TX_LOCK));
        if plan.install.is_empty() && plan.remove.is_empty() {
            return Ok(None);
        }

        // The initramfs trigger: sol-init of the new system as /init.
        let init_path = new.join(SOL_INIT);
        let init = fs::read(&init_path).at(&init_path)?;
        let img = new.join(INITRAMFS);
        let tmp = img.with_extension("img.new");
        let mut w = BufWriter::new(File::create(&tmp).at(&tmp)?);
        bootgen::initramfs::write(&mut w, &init).at(&tmp)?;
        w.flush().at(&tmp)?;
        drop(w);
        fs::rename(&tmp, &img).at(&img)?;

        let summary = summarize(&planned.0, &planned.1);
        let version = fs::read_to_string(new.join(OS_RELEASE))
            .ok()
            .and_then(|t| os_release_value(&t, "VERSION_ID"))
            .unwrap_or_default();
        let meta = Meta {
            id: id.to_string(),
            version,
            created: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            parent: parent.to_string(),
            summary: summary.clone(),
        };
        let text = toml::to_string(&meta).map_err(|e| Error::invalid(e.to_string()))?;
        write_atomic(&new.join(META), &text)?;
        Ok(Some(summary))
    }

    /// Makes `to` (default: the newest system older than the current
    /// default) the default from the next boot on. Returns its id.
    ///
    /// # Errors
    /// An unknown or unsealed system, nothing older to go back to, or a
    /// boot menu that cannot be written (it is then unchanged).
    pub fn rollback(&self, to: Option<&str>) -> Result<String> {
        let _lock = self.lock()?;
        let systems = self.list()?;
        let (mut settings, raw) = self.settings()?;
        let metas: Vec<&Meta> = systems
            .iter()
            .filter(|s| s.sealed)
            .filter_map(|s| s.meta.as_ref())
            .collect();
        let target = match to {
            Some(t) => {
                if !metas.iter().any(|m| m.id == t) {
                    return Err(Error::invalid(format!("no bootable system {t}")));
                }
                t.to_string()
            }
            None => rollback_target(&metas, settings.default.as_deref())
                .ok_or_else(|| Error::invalid("there is no older system to go back to"))?,
        };
        settings.default = Some(target.clone());
        let params = self.params(&systems, settings)?;
        self.write_boot_menu(&params)?;
        self.save_default(&raw, Some(&target))?;
        Ok(target)
    }

    /// The systems for `spk systems`: (metadata, booted, default).
    ///
    /// # Errors
    /// As [`Live::list`].
    pub fn overview(&self) -> Result<Vec<(Meta, bool, bool)>> {
        let systems = self.list()?;
        let (settings, _) = self.settings()?;
        let metas: Vec<&Meta> = systems
            .iter()
            .filter(|s| s.sealed)
            .filter_map(|s| s.meta.as_ref())
            .collect();
        let default = settings
            .default
            .clone()
            .or_else(|| newest(&metas).map(|m| m.id.clone()));
        let booted = self.booted_id().map(|b| b.to_string());
        let mut out: Vec<(Meta, bool, bool)> = metas
            .into_iter()
            .map(|m| {
                let is_booted = booted.as_deref() == Some(&m.id);
                let is_default = default.as_deref() == Some(&m.id);
                (m.clone(), is_booted, is_default)
            })
            .collect();
        out.sort_by_key(|o| std::cmp::Reverse(o.0.created));
        Ok(out)
    }
}

/// Failure injection for the QEMU tests (CLAUDE.md section 8): with
/// `SPK_TEST_PAUSE=<point>`, the transaction stops at that point and
/// waits, so the test can cut the power there. Test-only; unset, it does
/// nothing.
fn test_pause(point: &str, id: u64) {
    if std::env::var("SPK_TEST_PAUSE").is_ok_and(|p| p == point) {
        eprintln!("spk: test pause {point} (system {id})");
        std::thread::sleep(std::time::Duration::from_secs(600));
    }
}

/// A unix time as shown in the boot menu (`2026-10-19 09:30`, UTC).
pub fn format_date(secs: u64) -> String {
    bootgen::format_date(secs)
}

/// `@snapshots/7` -> 7.
fn subvol_id(subvol: &str) -> Option<u64> {
    subvol
        .strip_prefix(SNAPSHOTS)?
        .strip_prefix('/')?
        .parse()
        .ok()
}

/// The root filesystem's and the ESP's ids from the kernel command line.
fn boot_ids(cmdline: &str) -> Result<(String, String)> {
    let get = |key: &str| {
        cmdline
            .split_ascii_whitespace()
            .find_map(|w| w.strip_prefix(key))
            .and_then(|v| v.strip_prefix("UUID="))
            .map(str::to_string)
    };
    match (get("sol.root="), get("sol.esp=")) {
        (Some(r), Some(e)) => Ok((r, e)),
        _ => Err(Error::Conflict(
            "the kernel command line lacks sol.root=UUID= or sol.esp=UUID=; \
             system updates are unavailable"
                .into(),
        )),
    }
}

/// A value from os-release text (quotes removed).
fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let v = l.strip_prefix(key)?.strip_prefix('=')?;
        Some(v.trim().trim_matches('"').to_string())
    })
}

fn next_id(ids: impl Iterator<Item = u64>) -> u64 {
    ids.max().unwrap_or(0) + 1
}

fn newest<'a>(metas: &[&'a Meta]) -> Option<&'a Meta> {
    metas
        .iter()
        .copied()
        .max_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)))
}

/// The system to roll back to: the newest one older than the current
/// default (the explicit `default`, else the newest).
fn rollback_target(metas: &[&Meta], default: Option<&str>) -> Option<String> {
    let current = match default {
        Some(d) => metas.iter().copied().find(|m| m.id == d)?,
        None => newest(metas)?,
    };
    metas
        .iter()
        .copied()
        .filter(|m| (m.created, &m.id) < (current.created, &current.id))
        .max_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)))
        .map(|m| m.id.clone())
}

/// Sealed systems to delete: those the boot menu does not keep, never the
/// booted one.
fn prune_list(sealed: &[u64], kept: &[u64], booted: u64) -> Vec<u64> {
    sealed
        .iter()
        .copied()
        .filter(|id| *id != booted && !kept.contains(id))
        .collect()
}

fn summarize(install: &[String], remove: &[String]) -> String {
    let mut parts = Vec::new();
    if !install.is_empty() {
        parts.push(format!("install {}", install.join(", ")));
    }
    if !remove.is_empty() {
        parts.push(format!("remove {}", remove.join(", ")));
    }
    parts.join("; ")
}

/// Boot settings text with `default` set to `default`, or removed.
fn with_default(raw: &str, default: Option<&str>) -> Result<String> {
    let mut t: toml::Table = toml::from_str(raw).map_err(|e| Error::invalid(e.to_string()))?;
    match default {
        Some(d) => {
            t.insert("default".into(), toml::Value::String(d.into()));
        }
        None => {
            t.remove("default");
        }
    }
    toml::to_string(&t).map_err(|e| Error::invalid(e.to_string()))
}

/// Writes `path` atomically (temporary file, fsync, rename).
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).at(dir)?;
    }
    let tmp = path.with_extension("new");
    let mut f = File::create(&tmp).at(&tmp)?;
    f.write_all(text.as_bytes()).at(&tmp)?;
    f.sync_all().at(&tmp)?;
    drop(f);
    fs::rename(&tmp, path).at(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str, created: u64) -> Meta {
        Meta {
            id: id.into(),
            version: "0.1".into(),
            created,
            parent: String::new(),
            summary: String::new(),
        }
    }

    #[test]
    fn ids_and_subvolumes() {
        assert_eq!(next_id([1, 5, 3].into_iter()), 6);
        assert_eq!(next_id(std::iter::empty()), 1);
        assert_eq!(subvol_id("@snapshots/12"), Some(12));
        assert_eq!(subvol_id("@snapshots/x"), None);
        assert_eq!(subvol_id("@system"), None);
    }

    #[test]
    fn boot_ids_come_from_the_command_line() {
        let c = "BOOT_IMAGE=/sol/2/vmlinuz sol.root=UUID=0e7b-1 sol.system=@snapshots/2 sol.esp=UUID=AB12-CD34 rw quiet";
        assert_eq!(boot_ids(c).unwrap(), ("0e7b-1".into(), "AB12-CD34".into()));
        assert!(boot_ids("sol.root=UUID=x quiet").is_err());
        assert!(boot_ids("sol.root=/dev/vda2 sol.esp=UUID=y").is_err());
    }

    #[test]
    fn os_release_values() {
        let t = "NAME=\"Solace\"\nVERSION_ID=0.1\nPRETTY_NAME=\"Solace 0.1 (Koi)\"\n";
        assert_eq!(os_release_value(t, "NAME").as_deref(), Some("Solace"));
        assert_eq!(os_release_value(t, "VERSION_ID").as_deref(), Some("0.1"));
        assert_eq!(os_release_value(t, "VERSION"), None);
    }

    #[test]
    fn rollback_goes_to_the_system_before_the_default() {
        let (a, b, c) = (meta("1", 100), meta("2", 200), meta("3", 300));
        let all = [&a, &b, &c];
        assert_eq!(rollback_target(&all, None).as_deref(), Some("2"));
        assert_eq!(rollback_target(&all, Some("2")).as_deref(), Some("1"));
        assert_eq!(rollback_target(&all, Some("1")), None);
        assert_eq!(rollback_target(&all, Some("9")), None);
        assert_eq!(rollback_target(&[], None), None);
    }

    #[test]
    fn pruning_keeps_the_menu_and_the_booted_system() {
        assert_eq!(prune_list(&[1, 2, 3, 4, 5], &[5, 4, 3], 2), [1]);
        assert_eq!(prune_list(&[1, 2], &[2, 1], 1), Vec::<u64>::new());
    }

    #[test]
    fn default_is_set_and_cleared_keeping_other_settings() {
        let raw = "menu = \"always\"\ntimeout = 3\nconsole = [\"tty0\"]\n";
        let set = with_default(raw, Some("2")).unwrap();
        let s: bootgen::Settings = toml::from_str(&set).unwrap();
        assert_eq!(s.default.as_deref(), Some("2"));
        assert_eq!(s.timeout, 3);
        assert_eq!(s.console, ["tty0"]);
        let cleared: bootgen::Settings =
            toml::from_str(&with_default(&set, None).unwrap()).unwrap();
        assert_eq!(cleared.default, None);
        assert_eq!(cleared.menu, bootgen::MenuStyle::Always);
        assert_eq!(
            with_default("", Some("1")).unwrap().trim(),
            "default = \"1\""
        );
    }

    #[test]
    fn metadata_round_trips() {
        let m = Meta {
            summary: "install solace-base 0.1-2".into(),
            parent: "1".into(),
            ..meta("2", 1_792_454_400)
        };
        let back: Meta = toml::from_str(&toml::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn a_scratch_root_is_not_a_live_system() {
        let d = tempfile::tempdir().unwrap();
        assert!(Live::detect(d.path()).unwrap().is_none());
    }
}
