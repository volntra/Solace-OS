//! fern-bootgen: owns `grub.cfg` and the kernels on the ESP (CLAUDE.md 6.4).
//!
//! Security-critical. [`generate_cfg`] is a pure function of its inputs;
//! [`apply`] checks ESP space before copying anything, copies kernels via
//! temp files, prunes unlisted systems, and replaces `grub.cfg`
//! atomically (temp file, fsync, rename, fsync dir). If anything fails
//! before the rename, the previous `grub.cfg` is untouched.

pub mod initramfs;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Where everything we own lives on the ESP.
pub const FERN_DIR: &str = "fern";
pub const GRUB_CFG: &str = "grub/grub.cfg";
pub const THEME: &str = "grub/themes/fern/theme.txt";
/// Fonts the theme uses (boot/grub-theme/build.sh makes them).
pub const FONTS: &[&str] = &[
    "inter-regular-16.pf2",
    "inter-regular-20.pf2",
    "inter-semibold-24.pf2",
];
/// Kept free on the ESP after copying (firmware, other OSes, updates).
pub const ESP_RESERVE: u64 = 32 << 20;

#[derive(Debug)]
pub enum Error {
    Invalid(String),
    /// The ESP lacks room; nothing was changed.
    NoSpace {
        needed: u64,
        available: u64,
    },
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Invalid(m) | Error::Io(m) => f.write_str(m),
            Error::NoSpace { needed, available } => write!(
                f,
                "not enough space on the ESP: need {} MiB more, {} MiB available \
                 (keep fewer previous systems, or free space on the ESP); nothing was changed",
                needed.div_ceil(1 << 20),
                available >> 20
            ),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn io_err(p: &Path, e: &io::Error) -> Error {
    Error::Io(format!("{}: {e}", p.display()))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MenuStyle {
    /// Always show the menu for `timeout` seconds.
    Always,
    /// Boot straight away unless Shift is held (or Esc pressed).
    #[default]
    Hidden,
    /// Never show the menu.
    Never,
}

/// The boot settings domain (owned by fern-settingsd from Phase 6).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub menu: MenuStyle,
    #[serde(default = "default_timeout")]
    pub timeout: u32,
    /// Our gfxmenu theme (true) or plain text (false).
    #[serde(default = "default_true")]
    pub theme: bool,
    /// Kernel and init messages on screen instead of the splash.
    #[serde(default)]
    pub verbose: bool,
    /// Snapshot id to boot by default; newest if unset.
    #[serde(default)]
    pub default: Option<String>,
    /// How many systems stay bootable (newest first).
    #[serde(default = "default_keep")]
    pub keep: usize,
    /// Show entries for other operating systems.
    #[serde(default = "default_true")]
    pub other_os: bool,
    /// Kernel consoles (`console=`), e.g. `["tty0", "ttyS0,115200"]`; the
    /// last one is /dev/console. Empty: the kernel's default.
    #[serde(default)]
    pub console: Vec<String>,
}

fn default_timeout() -> u32 {
    5
}
fn default_true() -> bool {
    true
}
fn default_keep() -> usize {
    3
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            menu: MenuStyle::default(),
            timeout: default_timeout(),
            theme: true,
            verbose: false,
            default: None,
            keep: default_keep(),
            other_os: true,
            console: Vec::new(),
        }
    }
}

/// A bootable system snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct System {
    /// `[A-Za-z0-9._-]+`: becomes the directory `/boot/fern/<id>`.
    pub id: String,
    /// e.g. "0.1".
    pub version: String,
    /// Unix seconds.
    pub created: u64,
    /// The btrfs subvolume, e.g. `@snapshots/3`.
    pub subvol: String,
    /// Kernel and initramfs inside the build/host filesystem, to copy.
    pub kernel: PathBuf,
    pub initramfs: PathBuf,
}

/// Another OS's EFI loader (from os-prober).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OtherOs {
    pub title: String,
    /// Filesystem UUID of the partition holding the loader.
    pub uuid: String,
    /// e.g. `/EFI/Microsoft/Boot/bootmgfw.efi`.
    pub efi_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    /// FAT volume id of the ESP (`XXXX-XXXX`).
    pub esp_uuid: String,
    /// btrfs filesystem UUID holding the systems.
    pub root_uuid: String,
    pub systems: Vec<System>,
    pub other_os: Vec<OtherOs>,
    pub settings: Settings,
    /// Product name for entry titles.
    pub os_name: String,
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn valid_uuid(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// A kernel console spec: `tty0`, `ttyS0,115200n8`, `hvc0`.
fn valid_console(c: &str) -> bool {
    let (dev, opts) = c.split_once(',').unwrap_or((c, ""));
    !dev.is_empty()
        && dev.len() <= 16
        && dev.bytes().all(|b| b.is_ascii_alphanumeric())
        && opts.len() <= 16
        && opts.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Quotes a string for grub.cfg (single quotes; `'` becomes `'\''`).
/// Control characters are dropped.
pub fn grub_quote(s: &str) -> String {
    let clean: String = s.chars().filter(|c| !c.is_control()).collect();
    format!("'{}'", clean.replace('\'', r"'\''"))
}

impl Params {
    /// # Errors
    /// [`Error::Invalid`] for anything that could produce a broken or
    /// unsafe grub.cfg.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::Invalid(m));
        if !valid_uuid(&self.esp_uuid) || !valid_uuid(&self.root_uuid) {
            return bad("ESP and root UUIDs must be hex digits and dashes".into());
        }
        if self.systems.is_empty() {
            return bad("no bootable systems".into());
        }
        if self.settings.keep == 0 {
            return bad("keep must be at least 1".into());
        }
        if self.settings.timeout > 300 {
            return bad("timeout must be at most 300 seconds".into());
        }
        for c in &self.settings.console {
            if !valid_console(c) {
                return bad(format!("bad console {c:?}"));
            }
        }
        let mut ids = BTreeSet::new();
        for s in &self.systems {
            if !valid_id(&s.id) {
                return bad(format!("bad system id {:?}", s.id));
            }
            if !ids.insert(&s.id) {
                return bad(format!("system id {} listed twice", s.id));
            }
            let subvol_ok = !s.subvol.is_empty()
                && s.subvol
                    .split('/')
                    .all(|c| valid_id(c) || c.starts_with('@') && valid_id(&c[1..]));
            if !subvol_ok {
                return bad(format!("bad subvolume {:?}", s.subvol));
            }
            if !valid_id(&s.version.replace(' ', "_")) {
                return bad(format!("bad version {:?}", s.version));
            }
        }
        if let Some(d) = &self.settings.default
            && !self.systems.iter().any(|s| &s.id == d)
        {
            return bad(format!("default system {d} does not exist"));
        }
        for o in &self.other_os {
            if !valid_uuid(&o.uuid) {
                return bad(format!("bad UUID for {}", o.title));
            }
            let path_ok =
                o.efi_path.starts_with('/') && o.efi_path.split('/').skip(1).all(valid_id);
            if !path_ok {
                return bad(format!("bad EFI path {:?}", o.efi_path));
            }
        }
        Ok(())
    }

    /// The systems that stay bootable: newest first, at most `keep`, and
    /// always including the default.
    pub fn kept(&self) -> Vec<&System> {
        let mut v: Vec<&System> = self.systems.iter().collect();
        v.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
        let mut kept: Vec<&System> = v.iter().take(self.settings.keep).copied().collect();
        if let Some(d) = &self.settings.default
            && !kept.iter().any(|s| &s.id == d)
            && let Some(s) = v.iter().find(|s| &s.id == d)
        {
            kept.pop();
            kept.push(s);
        }
        kept
    }

    fn default_id(&self) -> String {
        self.settings
            .default
            .clone()
            .unwrap_or_else(|| self.kept()[0].id.clone())
    }
}

/// `2026-10-19 14:05` (UTC) from Unix seconds.
pub fn format_date(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// The full grub.cfg text. Pure.
///
/// # Errors
/// Invalid parameters.
pub fn generate_cfg(p: &Params) -> Result<String> {
    p.validate()?;
    let s = &p.settings;
    let mut c = String::new();
    let w = &mut c;
    let _ = writeln!(
        w,
        "# Generated by fern-bootgen. Do not edit: changes are overwritten."
    );
    let _ = writeln!(w, "# Boot settings live in Settings > Startup & Boot.\n");
    let _ = writeln!(
        w,
        "insmod part_gpt\ninsmod fat\ninsmod all_video\ninsmod gfxterm\ninsmod png"
    );
    let _ = writeln!(w, "search --no-floppy --fs-uuid --set=root {}", p.esp_uuid);
    let _ = writeln!(
        w,
        "set default={}",
        grub_quote(&format!("fern-{}", p.default_id()))
    );
    let _ = writeln!(w, "set gfxmode=auto\nset gfxpayload=keep");
    if s.theme {
        let _ = writeln!(w, "terminal_output gfxterm");
        for font in FONTS {
            let _ = writeln!(w, "loadfont ($root)/grub/fonts/{font}");
        }
        let _ = writeln!(w, "set theme=($root)/{THEME}");
    } else {
        let _ = writeln!(w, "terminal_output console");
        let _ = writeln!(w, "set menu_color_normal=light-gray/black");
        let _ = writeln!(w, "set menu_color_highlight=black/light-gray");
    }
    match s.menu {
        MenuStyle::Always => {
            let _ = writeln!(w, "set timeout_style=menu\nset timeout={}", s.timeout);
        }
        MenuStyle::Hidden => {
            // Shift held: show the menu and wait. Otherwise boot at once;
            // Esc during the short hidden timeout also shows the menu.
            let _ = writeln!(
                w,
                "if keystatus --shift; then\n  set timeout_style=menu\n  set timeout=-1\n\
                 else\n  set timeout_style=hidden\n  set timeout=1\nfi"
            );
        }
        MenuStyle::Never => {
            let _ = writeln!(w, "set timeout_style=hidden\nset timeout=0");
        }
    }
    let mut cmdline_extra = String::from(if s.verbose {
        "fern.verbose loglevel=7"
    } else {
        "quiet loglevel=3 rd.udev.log_level=3 vt.global_cursor_default=0"
    });
    for c in &s.console {
        let _ = write!(cmdline_extra, " console={c}");
    }
    for sys in p.kept() {
        let title = format!(
            "{} {} \u{2014} {}",
            p.os_name,
            sys.version,
            format_date(sys.created)
        );
        let _ = writeln!(
            w,
            "\nmenuentry {} --class fern --id {} {{",
            grub_quote(&title),
            grub_quote(&format!("fern-{}", sys.id))
        );
        let _ = writeln!(w, "  set gfxpayload=keep\n  insmod gzio");
        let _ = writeln!(
            w,
            "  linux /{FERN_DIR}/{id}/vmlinuz fern.root=UUID={root} fern.system={subvol} fern.esp=UUID={esp} rw {cmdline_extra}",
            id = sys.id,
            root = p.root_uuid,
            subvol = sys.subvol,
            esp = p.esp_uuid
        );
        let _ = writeln!(w, "  initrd /{FERN_DIR}/{}/initramfs.img", sys.id);
        let _ = writeln!(w, "}}");
    }
    if s.other_os {
        for o in &p.other_os {
            let _ = writeln!(
                w,
                "\nmenuentry {} --class os {{\n  insmod chain\n  search --no-floppy --fs-uuid --set=root {}\n  chainloader {}\n}}",
                grub_quote(&o.title),
                o.uuid,
                o.efi_path
            );
        }
    }
    Ok(c)
}

/// Free bytes on a filesystem (the real implementation of the space check).
///
/// # Errors
/// statvfs failure.
pub fn free_bytes(path: &Path) -> Result<u64> {
    let st = rustix::fs::statvfs(path).map_err(|e| io_err(path, &e.into()))?;
    Ok(st.f_bavail.saturating_mul(st.f_frsize))
}

fn file_hash(p: &Path) -> Result<[u8; 32]> {
    let mut f = File::open(p).map_err(|e| io_err(p, &e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| io_err(p, &e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

/// What [`apply`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub copied: Vec<String>,
    pub removed: Vec<String>,
    pub entries: usize,
}

fn write_atomic(target: &Path, data: &[u8]) -> Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| Error::Invalid(format!("{} has no parent", target.display())))?;
    fs::create_dir_all(dir).map_err(|e| io_err(dir, &e))?;
    let tmp = dir.join(format!(
        ".{}.new",
        target.file_name().and_then(|n| n.to_str()).unwrap_or("tmp")
    ));
    {
        let mut f = File::create(&tmp).map_err(|e| io_err(&tmp, &e))?;
        f.write_all(data).map_err(|e| io_err(&tmp, &e))?;
        f.sync_all().map_err(|e| io_err(&tmp, &e))?;
    }
    fs::rename(&tmp, target).map_err(|e| io_err(target, &e))?;
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

fn copy_atomic(src: &Path, dst: &Path) -> Result<()> {
    let data = fs::read(src).map_err(|e| io_err(src, &e))?;
    write_atomic(dst, &data)
}

/// Updates the ESP mounted at `esp` for `p`. `free` reports free bytes
/// (injected for tests; [`free_bytes`] in production).
///
/// # Errors
/// Invalid parameters, missing kernels, [`Error::NoSpace`], I/O errors. On
/// every error before the final rename, grub.cfg is unchanged.
pub fn apply(esp: &Path, p: &Params, free: &dyn Fn(&Path) -> Result<u64>) -> Result<Report> {
    let cfg = generate_cfg(p)?;
    let kept = p.kept();
    let fern = esp.join(FERN_DIR);

    // 1. Work out which files must be copied, and check space for them.
    let mut todo: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    let mut needed: u64 = 0;
    for sys in &kept {
        for (src, name) in [(&sys.kernel, "vmlinuz"), (&sys.initramfs, "initramfs.img")] {
            let meta = fs::metadata(src).map_err(|e| io_err(src, &e))?;
            if !meta.is_file() {
                return Err(Error::Invalid(format!("{} is not a file", src.display())));
            }
            let dst = fern.join(&sys.id).join(name);
            let same = dst.exists() && file_hash(&dst)? == file_hash(src)?;
            if !same {
                // A replaced file needs room for the new copy before the
                // old one goes, so count its full size.
                needed = needed.saturating_add(meta.len());
                todo.push((src.clone(), dst, format!("{}/{name}", sys.id)));
            }
        }
    }
    needed = needed.saturating_add(cfg.len() as u64);
    let available = free(esp)?;
    if needed.saturating_add(ESP_RESERVE) > available {
        return Err(Error::NoSpace {
            needed: needed.saturating_add(ESP_RESERVE).saturating_sub(available),
            available,
        });
    }

    // 2. Copy kernels and initramfs images (each via a temp file).
    let mut report = Report::default();
    for (src, dst, label) in todo {
        copy_atomic(&src, &dst)?;
        report.copied.push(label);
    }

    // 3. The new grub.cfg, atomically. Only after this do we prune: the old
    //    config may still reference systems we are about to remove.
    write_atomic(&esp.join(GRUB_CFG), cfg.as_bytes())?;
    report.entries = kept.len();

    // 4. Remove systems that are no longer bootable.
    let keep_ids: BTreeSet<&str> = kept.iter().map(|s| s.id.as_str()).collect();
    if let Ok(rd) = fs::read_dir(&fern) {
        let mut stale: Vec<String> = rd
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|n| valid_id(n) && !keep_ids.contains(n.as_str()))
            .collect();
        stale.sort();
        for n in stale {
            let d = fern.join(&n);
            fs::remove_dir_all(&d).map_err(|e| io_err(&d, &e))?;
            report.removed.push(n);
        }
    }
    Ok(report)
}

/// Input file for the CLI: the systems and other-OS entries.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub esp_uuid: String,
    pub root_uuid: String,
    #[serde(default = "default_os_name")]
    pub os_name: String,
    #[serde(rename = "system")]
    pub systems: Vec<System>,
    #[serde(default, rename = "other_os")]
    pub other_os: Vec<OtherOs>,
}

fn default_os_name() -> String {
    "Fern".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(id: u32, created: u64, dir: &Path) -> System {
        let k = dir.join(format!("k{id}"));
        let i = dir.join(format!("i{id}"));
        fs::write(&k, format!("kernel {id}")).unwrap();
        fs::write(&i, format!("initramfs {id}")).unwrap();
        System {
            id: id.to_string(),
            version: "0.1".into(),
            created,
            subvol: format!("@snapshots/{id}"),
            kernel: k,
            initramfs: i,
        }
    }

    fn params(n: u32, dir: &Path) -> Params {
        Params {
            esp_uuid: "ABCD-1234".into(),
            root_uuid: "0e7b1b1c-2d33-4c6e-9a7e-1f2b3c4d5e6f".into(),
            systems: (1..=n)
                .map(|i| sys(i, 1_792_000_000 + u64::from(i) * 3600, dir))
                .collect(),
            other_os: vec![],
            settings: Settings {
                keep: 100,
                ..Settings::default()
            },
            os_name: "Fern".into(),
        }
    }

    // Must match the injected free-space function's signature.
    #[allow(clippy::unnecessary_wraps)]
    fn plenty(_: &Path) -> Result<u64> {
        Ok(1 << 30)
    }

    #[test]
    fn configs_for_1_5_and_20_systems() {
        let d = tempfile::tempdir().unwrap();
        for n in [1u32, 5, 20] {
            let p = params(n, d.path());
            let cfg = generate_cfg(&p).unwrap();
            assert_eq!(cfg.matches("\nmenuentry ").count(), n as usize, "{n}");
            // Newest first, and the default is the newest.
            let first = cfg.find(&format!("--id 'fern-{n}'")).unwrap();
            if n > 1 {
                assert!(first < cfg.find("--id 'fern-1'").unwrap());
            }
            assert!(cfg.contains(&format!("set default='fern-{n}'")));
            assert!(cfg.contains(&format!(
                "linux /fern/{n}/vmlinuz fern.root=UUID={} fern.system=@snapshots/{n} fern.esp=UUID={} rw",
                p.root_uuid, p.esp_uuid
            )));
            assert!(!cfg.contains("GNU"), "no upstream branding");
        }
    }

    #[test]
    fn keep_limits_entries_but_keeps_the_default() {
        let d = tempfile::tempdir().unwrap();
        let mut p = params(20, d.path());
        p.settings.keep = 3;
        assert_eq!(generate_cfg(&p).unwrap().matches("\nmenuentry ").count(), 3);
        p.settings.default = Some("2".into());
        let kept: Vec<&str> = p.kept().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(kept, ["20", "19", "2"]);
        let cfg = generate_cfg(&p).unwrap();
        assert!(cfg.contains("set default='fern-2'"));
    }

    #[test]
    fn menu_styles() {
        let d = tempfile::tempdir().unwrap();
        let mut p = params(1, d.path());
        p.settings.menu = MenuStyle::Always;
        p.settings.timeout = 7;
        let c = generate_cfg(&p).unwrap();
        assert!(c.contains("set timeout_style=menu\nset timeout=7"));
        p.settings.menu = MenuStyle::Hidden;
        let c = generate_cfg(&p).unwrap();
        assert!(c.contains("keystatus --shift") && c.contains("set timeout=-1"));
        p.settings.menu = MenuStyle::Never;
        let c = generate_cfg(&p).unwrap();
        assert!(c.contains("set timeout=0") && !c.contains("keystatus"));
    }

    #[test]
    fn consoles_go_on_the_kernel_line_and_are_validated() {
        let d = tempfile::tempdir().unwrap();
        let mut p = params(1, d.path());
        p.settings.console = vec!["tty0".into(), "ttyS0,115200".into()];
        let c = generate_cfg(&p).unwrap();
        assert!(c.contains(" console=tty0 console=ttyS0,115200\n"));
        for bad in [
            "",
            "tty0 init=/bin/sh",
            "ttyS0,1;reboot",
            "tty0\nlinux",
            "../x",
        ] {
            p.settings.console = vec![bad.into()];
            assert!(generate_cfg(&p).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn theme_and_verbose() {
        let d = tempfile::tempdir().unwrap();
        let mut p = params(1, d.path());
        let c = generate_cfg(&p).unwrap();
        assert!(c.contains("set theme=($root)/grub/themes/fern/theme.txt"));
        assert!(c.contains(" quiet ") && c.contains("gfxpayload=keep"));
        p.settings.theme = false;
        p.settings.verbose = true;
        let c = generate_cfg(&p).unwrap();
        assert!(!c.contains("set theme") && c.contains("terminal_output console"));
        assert!(c.contains("fern.verbose") && !c.contains(" quiet "));
    }

    #[test]
    fn other_os_entries() {
        let d = tempfile::tempdir().unwrap();
        let mut p = params(2, d.path());
        p.other_os = vec![OtherOs {
            title: "Windows Boot Manager".into(),
            uuid: "1234-ABCD".into(),
            efi_path: "/EFI/Microsoft/Boot/bootmgfw.efi".into(),
        }];
        let c = generate_cfg(&p).unwrap();
        assert!(c.contains("menuentry 'Windows Boot Manager' --class os"));
        assert!(c.contains("chainloader /EFI/Microsoft/Boot/bootmgfw.efi"));
        assert!(c.find("Windows").unwrap() > c.rfind("--class fern").unwrap());
        p.settings.other_os = false;
        assert!(!generate_cfg(&p).unwrap().contains("Windows"));
    }

    #[test]
    fn hostile_input_cannot_break_out() {
        let d = tempfile::tempdir().unwrap();
        let mut p = params(1, d.path());
        p.other_os = vec![OtherOs {
            title: "evil'; rmmod all; echo '\n}".into(),
            uuid: "1234-ABCD".into(),
            efi_path: "/EFI/x/y.efi".into(),
        }];
        let c = generate_cfg(&p).unwrap();
        assert!(c.contains(r"menuentry 'evil'\''; rmmod all; echo '\''}' --class os"));
        for (field, bad) in [
            ("id", "../../x"),
            ("subvol", "@snapshots/../../etc"),
            ("efi", "/EFI/../../x.efi"),
            ("uuid", "x; reboot"),
            ("version", "0.1\nmenuentry"),
        ] {
            let mut q = params(1, d.path());
            match field {
                "id" => q.systems[0].id = bad.into(),
                "subvol" => q.systems[0].subvol = bad.into(),
                "efi" => q.other_os.push(OtherOs {
                    title: "t".into(),
                    uuid: "1".into(),
                    efi_path: bad.into(),
                }),
                "uuid" => q.root_uuid = bad.into(),
                _ => q.systems[0].version = bad.into(),
            }
            assert!(generate_cfg(&q).is_err(), "{field}: {bad}");
        }
    }

    #[test]
    fn validation() {
        let d = tempfile::tempdir().unwrap();
        let mut p = params(2, d.path());
        p.settings.default = Some("9".into());
        assert!(generate_cfg(&p).is_err());
        let mut p = params(2, d.path());
        p.systems[1].id = "1".into();
        assert!(generate_cfg(&p).unwrap_err().to_string().contains("twice"));
        let mut p = params(1, d.path());
        p.systems.clear();
        assert!(generate_cfg(&p).is_err());
        let mut p = params(1, d.path());
        p.settings.keep = 0;
        assert!(generate_cfg(&p).is_err());
    }

    #[test]
    fn apply_copies_prunes_and_is_idempotent() {
        let src = tempfile::tempdir().unwrap();
        let esp = tempfile::tempdir().unwrap();
        let mut p = params(5, src.path());
        p.settings.keep = 5;
        let r = apply(esp.path(), &p, &plenty).unwrap();
        assert_eq!(r.copied.len(), 10);
        assert_eq!(r.entries, 5);
        assert_eq!(
            fs::read_to_string(esp.path().join("fern/3/vmlinuz")).unwrap(),
            "kernel 3"
        );
        assert!(esp.path().join(GRUB_CFG).exists());
        assert!(!esp.path().join("grub/.grub.cfg.new").exists());

        // Unchanged kernels are not copied again.
        let r = apply(esp.path(), &p, &plenty).unwrap();
        assert!(r.copied.is_empty());

        // Keep fewer: old systems leave the ESP, after the config changed.
        p.settings.keep = 2;
        let r = apply(esp.path(), &p, &plenty).unwrap();
        assert_eq!(r.removed, ["1", "2", "3"]);
        assert!(!esp.path().join("fern/1").exists());
        let cfg = fs::read_to_string(esp.path().join(GRUB_CFG)).unwrap();
        assert!(!cfg.contains("fern-1'"));

        // A rebuilt kernel is replaced.
        fs::write(&p.systems[4].kernel, "kernel 5 rebuilt").unwrap();
        let r = apply(esp.path(), &p, &plenty).unwrap();
        assert_eq!(r.copied, ["5/vmlinuz"]);
    }

    #[test]
    fn nearly_full_esp_changes_nothing() {
        let src = tempfile::tempdir().unwrap();
        let esp = tempfile::tempdir().unwrap();
        let p1 = params(1, src.path());
        apply(esp.path(), &p1, &plenty).unwrap();
        let before = fs::read_to_string(esp.path().join(GRUB_CFG)).unwrap();

        let p2 = params(3, src.path());
        let tight = |_: &Path| -> Result<u64> { Ok(ESP_RESERVE + 10) };
        let e = apply(esp.path(), &p2, &tight).unwrap_err();
        assert!(matches!(e, Error::NoSpace { .. }), "{e}");
        assert!(e.to_string().contains("nothing was changed"));
        assert_eq!(
            fs::read_to_string(esp.path().join(GRUB_CFG)).unwrap(),
            before
        );
        assert!(!esp.path().join("fern/3").exists());
    }

    #[test]
    fn missing_kernel_changes_nothing() {
        let src = tempfile::tempdir().unwrap();
        let esp = tempfile::tempdir().unwrap();
        let p = params(2, src.path());
        apply(esp.path(), &p, &plenty).unwrap();
        let before = fs::read_to_string(esp.path().join(GRUB_CFG)).unwrap();
        let mut p3 = params(3, src.path());
        fs::remove_file(&p3.systems[2].initramfs).unwrap();
        p3.settings.keep = 3;
        assert!(apply(esp.path(), &p3, &plenty).is_err());
        assert_eq!(
            fs::read_to_string(esp.path().join(GRUB_CFG)).unwrap(),
            before
        );
    }

    #[test]
    fn dates() {
        assert_eq!(format_date(0), "1970-01-01 00:00");
        assert_eq!(format_date(1_792_368_000), "2026-10-19 00:00");
        assert_eq!(format_date(951_782_400 + 3661), "2000-02-29 01:01");
    }

    #[test]
    fn quoting() {
        assert_eq!(grub_quote("Fern"), "'Fern'");
        assert_eq!(grub_quote("it's"), r"'it'\''s'");
        assert_eq!(grub_quote("a\nb\tc"), "'abc'");
    }
}
