//! What only PID 1 does: early mounts, assembling the root from btrfs
//! subvolumes (when started from the initramfs), first-boot /etc seeding,
//! hostname, random seed, sysctls, and the final unmount + reboot.
//!
//! Security-critical: runs as root before anything else. Every path it
//! touches is fixed; the only external input is the kernel command line.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustix::mount::{MountFlags, UnmountFlags, mount, mount_move, mount_remount, unmount};

use crate::log::LogStore;
use crate::runtime::Power;

/// Where the real sol-init lives inside the system image.
pub const INIT_PATH: &str = "/usr/lib/sol/sol-init";
const RANDOM_SEED: &str = "/var/lib/sol/random-seed";
const SEED_BYTES: usize = 512;

/// Kernel command line options we use (docs/formats/cmdline.md).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cmdline {
    /// `sol.root=UUID=<btrfs fsid>` or `sol.root=/dev/...`.
    pub root: Option<String>,
    /// `sol.system=<subvolume>`: the @system snapshot to boot.
    pub system: Option<String>,
    /// `sol.esp=UUID=<FAT volume id>`: the ESP, mounted at /boot.
    pub esp: Option<String>,
    /// `sol.verbose`: echo sol-log to the console.
    pub verbose: bool,
}

impl Cmdline {
    pub fn parse(text: &str) -> Self {
        let mut c = Cmdline::default();
        for w in text.split_ascii_whitespace() {
            if let Some(v) = w.strip_prefix("sol.root=") {
                c.root = Some(v.to_string());
            } else if let Some(v) = w.strip_prefix("sol.system=") {
                c.system = Some(v.to_string());
            } else if let Some(v) = w.strip_prefix("sol.esp=") {
                c.esp = Some(v.to_string());
            } else if w == "sol.verbose" {
                c.verbose = true;
            }
        }
        c
    }
}

/// mount(2) options as the C string rustix wants (empty -> none).
fn opts(data: &str) -> std::ffi::CString {
    // Our option strings are constants without NUL bytes.
    std::ffi::CString::new(data).unwrap_or_default()
}

fn mnt(src: &str, target: &str, fstype: &str, flags: MountFlags, data: &str) {
    let _ = fs::create_dir_all(target);
    let o = opts(data);
    if let Err(e) = mount(src, target, fstype, flags, Some(o.as_c_str())) {
        // EBUSY: already mounted (e.g. by the kernel or the initramfs).
        if e != rustix::io::Errno::BUSY {
            eprintln!("solace: mount {target}: {e}");
        }
    }
}

/// The API filesystems every process expects.
pub fn early_mounts() {
    let nosuid_nodev_noexec = MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC;
    mnt("proc", "/proc", "proc", nosuid_nodev_noexec, "");
    mnt("sysfs", "/sys", "sysfs", nosuid_nodev_noexec, "");
    mnt(
        "devtmpfs",
        "/dev",
        "devtmpfs",
        MountFlags::NOSUID,
        "mode=0755",
    );
    mnt(
        "devpts",
        "/dev/pts",
        "devpts",
        MountFlags::NOSUID | MountFlags::NOEXEC,
        "gid=5,mode=0620,ptmxmode=000",
    );
    mnt(
        "tmpfs",
        "/dev/shm",
        "tmpfs",
        MountFlags::NOSUID | MountFlags::NODEV,
        "mode=1777",
    );
    mnt(
        "tmpfs",
        "/run",
        "tmpfs",
        MountFlags::NOSUID | MountFlags::NODEV,
        "mode=0755",
    );
    mnt(
        "cgroup2",
        "/sys/fs/cgroup",
        "cgroup2",
        nosuid_nodev_noexec,
        "nsdelegate",
    );
    mnt(
        "tmpfs",
        "/tmp",
        "tmpfs",
        MountFlags::NOSUID | MountFlags::NODEV,
        "mode=1777",
    );
}

/// Reads the btrfs filesystem UUID of a block device (superblock at
/// 64 KiB: fsid at +0x20, magic `_BHRfS_M` at +0x40).
pub fn btrfs_fsid(dev: &Path) -> Option<String> {
    let mut f = fs::File::open(dev).ok()?;
    let mut sb = [0u8; 0x48];
    f.seek(SeekFrom::Start(0x10000)).ok()?;
    f.read_exact(&mut sb).ok()?;
    if &sb[0x40..0x48] != b"_BHRfS_M" {
        return None;
    }
    let id = &sb[0x20..0x30];
    let h: String = id.iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{b:02x}");
        acc
    });
    Some(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

/// Reads the volume id of a FAT12/16/32 filesystem as `XXXX-XXXX` (what
/// GRUB, blkid and our bootgen call its UUID).
pub fn fat_volume_id(dev: &Path) -> Option<String> {
    let mut f = fs::File::open(dev).ok()?;
    let mut bs = [0u8; 512];
    f.read_exact(&mut bs).ok()?;
    if bs[510..512] != [0x55, 0xAA] {
        return None;
    }
    let at = if &bs[0x52..0x57] == b"FAT32" {
        0x43
    } else if &bs[0x36..0x39] == b"FAT" {
        0x27
    } else {
        return None;
    };
    let id = u32::from_le_bytes([bs[at], bs[at + 1], bs[at + 2], bs[at + 3]]);
    Some(format!("{:04X}-{:04X}", id >> 16, id & 0xFFFF))
}

/// Finds the block device named by `spec` (`UUID=<id>` as read by
/// `probe`, or a path), waiting up to 10 s for it to appear: devtmpfs
/// creates nodes as drivers probe.
fn find_device(spec: &str, probe: fn(&Path) -> Option<String>) -> Option<PathBuf> {
    for _ in 0..100 {
        if let Some(uuid) = spec.strip_prefix("UUID=") {
            if let Ok(rd) = fs::read_dir("/sys/class/block") {
                for e in rd.flatten() {
                    let dev = Path::new("/dev").join(e.file_name());
                    if probe(&dev).is_some_and(|id| id.eq_ignore_ascii_case(uuid)) {
                        return Some(dev);
                    }
                }
            }
        } else if Path::new(spec).exists() {
            return Some(PathBuf::from(spec));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

/// Finds the root device named by `sol.root=`.
pub fn find_root(spec: &str) -> Option<PathBuf> {
    find_device(spec, btrfs_fsid)
}

/// Where spk finds the btrfs top level (docs/formats/system.md).
pub const BTRFS_TOP: &str = "/run/sol/btrfs";
/// The booted system's subvolume, written at boot for spk.
pub const BOOTED_FILE: &str = "/run/sol/booted";

/// In the real root: mounts what system transactions need (the btrfs top
/// level under the root-only /run/sol, the ESP at /boot) and records the
/// booted system. Failures are logged; the system still boots.
pub fn mount_system_volumes(c: &Cmdline, log: &mut LogStore) {
    let mut note = |m: String| {
        eprintln!("solace: {m}");
        log.push("sol-init", "init", &m);
    };
    let _ = fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create("/run/sol");
    if let Some(dev) = c.root.as_deref().and_then(find_root) {
        let _ = fs::create_dir_all(BTRFS_TOP);
        let o = opts("subvolid=5,compress=zstd:3");
        let flags =
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC | MountFlags::NOATIME;
        if let Err(e) = mount(&dev, BTRFS_TOP, "btrfs", flags, Some(o.as_c_str())) {
            note(format!("mount the system volume at {BTRFS_TOP}: {e}"));
        }
    } else {
        note("no root device: system updates are unavailable".into());
    }
    if let Some(system) = &c.system
        && let Err(e) = fs::write(BOOTED_FILE, system)
    {
        note(format!("{BOOTED_FILE}: {e}"));
    }
    match c.esp.as_deref().map(|s| (s, find_device(s, fat_volume_id))) {
        Some((_, Some(dev))) => {
            let o = opts("umask=0077,shortname=mixed,utf8");
            let flags = MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC;
            if let Err(e) = mount(&dev, "/boot", "vfat", flags, Some(o.as_c_str())) {
                note(format!("mount the ESP at /boot: {e}"));
            }
        }
        Some((spec, None)) => note(format!("ESP {spec} did not appear")),
        None => note("no sol.esp= on the kernel command line".into()),
    }
}

/// The directories and links of a Solace root (CLAUDE.md section 4).
pub const ROOT_DIRS: &[&str] = &[
    "usr",
    "etc",
    "var",
    "Users",
    "Applications",
    "Volumes",
    "boot",
    "dev",
    "proc",
    "sys",
    "run",
    "tmp",
    "root",
    "mnt",
];
pub const ROOT_LINKS: &[(&str, &str)] = &[
    ("bin", "usr/bin"),
    ("sbin", "usr/sbin"),
    ("lib", "usr/lib"),
    ("lib64", "usr/lib"),
    ("home", "Users"),
    ("System", "usr"),
];

/// btrfs options and mount flags for one of our subvolumes. Generic words
/// such as `noatime` are flags, not filesystem options: mount(2) passes
/// the data string to btrfs, which rejects them with EINVAL.
fn subvol_mount(name: &str) -> (String, MountFlags) {
    (
        format!("subvol={name},compress=zstd:3"),
        MountFlags::NOATIME,
    )
}

/// Builds the real root under `new_root` from the btrfs subvolumes on
/// `dev`: a tmpfs skeleton with `@system` (read-only) at /usr, `@etc`,
/// `@var`, and `@users`.
///
/// # Errors
/// Mount failures (as text, for the console).
pub fn assemble_root(dev: &Path, system: &str, new_root: &Path) -> Result<(), String> {
    let r = |p: &str| new_root.join(p);
    let dev_s = dev.to_string_lossy();
    fs::create_dir_all(new_root).map_err(|e| e.to_string())?;
    let o = opts("mode=0755");
    mount(
        "tmpfs",
        new_root,
        "tmpfs",
        MountFlags::empty(),
        Some(o.as_c_str()),
    )
    .map_err(|e| format!("tmpfs root: {e}"))?;
    for d in ROOT_DIRS {
        fs::create_dir_all(r(d)).map_err(|e| e.to_string())?;
    }
    for (link, target) in ROOT_LINKS {
        std::os::unix::fs::symlink(target, r(link)).map_err(|e| e.to_string())?;
    }
    let subvol = |name: &str, target: &str, flags: MountFlags| -> Result<(), String> {
        let (data, base) = subvol_mount(name);
        let o = opts(&data);
        mount(
            &*dev_s,
            r(target),
            "btrfs",
            base | flags,
            Some(o.as_c_str()),
        )
        .map_err(|e| format!("mount {name} at /{target}: {e}"))
    };
    subvol(system, "usr", MountFlags::RDONLY)?;
    subvol("@etc", "etc", MountFlags::empty())?;
    subvol("@var", "var", MountFlags::empty())?;
    subvol("@users", "Users", MountFlags::NOSUID | MountFlags::NODEV)?;
    Ok(())
}

/// Moves the API mounts into `new_root`, makes it `/`, and execs the real
/// sol-init (which stays PID 1).
pub fn switch_root(new_root: &Path) -> std::io::Error {
    for m in ["/dev", "/proc", "/sys", "/run"] {
        let target = new_root.join(m.trim_start_matches('/'));
        if let Err(e) = mount_move(m, &target) {
            eprintln!("solace: moving {m}: {e}");
        }
    }
    if let Err(e) = std::env::set_current_dir(new_root) {
        return e;
    }
    if let Err(e) = mount_move(".", "/") {
        return e.into();
    }
    if let Err(e) = rustix::process::chroot(".") {
        return e.into();
    }
    if let Err(e) = std::env::set_current_dir("/") {
        return e;
    }
    Command::new(INIT_PATH).exec()
}

/// Copies `/usr/share/factory/etc` into an empty `/etc` on first boot.
pub fn seed_etc(root: &Path, log: &mut LogStore) {
    let etc = root.join("etc");
    let marker = etc.join(".sol-seeded");
    if marker.exists() {
        return;
    }
    let factory = root.join("usr/share/factory/etc");
    let mut n = 0;
    if let Ok(walk) = walk(&factory) {
        for src in walk {
            let rel = src.strip_prefix(&factory).unwrap_or(&src);
            let dst = etc.join(rel);
            if dst.exists() {
                continue;
            }
            let ok = if src.is_symlink() {
                fs::read_link(&src).and_then(|t| std::os::unix::fs::symlink(t, &dst))
            } else if src.is_dir() {
                fs::create_dir_all(&dst)
            } else {
                fs::copy(&src, &dst).map(|_| ())
            };
            if ok.is_ok() {
                n += 1;
            }
        }
    }
    let _ = fs::write(&marker, "");
    log.push(
        "sol-init",
        "init",
        &format!("first boot: seeded /etc with {n} entries"),
    );
}

fn walk(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&d)?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            out.push(p.clone());
            if p.is_dir() && !p.is_symlink() {
                stack.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

pub fn set_hostname(log: &mut LogStore) {
    let name = fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 64)
        .unwrap_or_else(|| "solace".into());
    if let Err(e) = rustix::system::sethostname(name.as_bytes()) {
        log.push("sol-init", "init", &format!("sethostname: {e}"));
    }
}

/// Mixes the saved seed into the kernel pool (no entropy credit).
pub fn load_random_seed() {
    if let Ok(seed) = fs::read(RANDOM_SEED) {
        let _ = fs::write("/dev/urandom", seed);
    }
}

pub fn save_random_seed() {
    let mut buf = vec![0u8; SEED_BYTES];
    if fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok()
    {
        let _ = fs::create_dir_all("/var/lib/sol");
        let _ = fs::write(RANDOM_SEED, &buf);
        let _ = fs::set_permissions(
            RANDOM_SEED,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        );
    }
}

/// Parses sysctl.d lines (`key = value`, `#`/`;` comments) into
/// (`/proc/sys` path, value) pairs.
pub fn parse_sysctl(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().trim_start_matches('-'), v.trim()))
        .filter(|(k, _)| {
            !k.is_empty()
                && k.split(['.', '/'])
                    .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains('\0'))
        })
        .map(|(k, v)| (format!("/proc/sys/{}", k.replace('.', "/")), v.to_string()))
        .collect()
}

/// Applies `/usr/lib/sysctl.d/*.conf` then `/etc/sysctl.d/*.conf`.
pub fn apply_sysctl(log: &mut LogStore) {
    for dir in ["/usr/lib/sysctl.d", "/etc/sysctl.d"] {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files
            .iter()
            .filter(|f| f.extension().is_some_and(|e| e == "conf"))
        {
            for (path, val) in parse_sysctl(&fs::read_to_string(f).unwrap_or_default()) {
                if let Err(e) = fs::write(&path, &val) {
                    log.push("sol-init", "init", &format!("sysctl {path}: {e}"));
                }
            }
        }
    }
}

/// Every process except PID 1 and kernel threads.
fn other_pids() -> Vec<rustix::process::Pid> {
    let Ok(rd) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|p| *p > 1)
        // Kernel threads have an empty cmdline.
        .filter(|p| fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| !c.is_empty()))
        .filter_map(rustix::process::Pid::from_raw)
        .collect()
}

/// Final steps after every unit has stopped. Never returns.
pub fn finish(power: Power, log: &mut LogStore) -> ! {
    save_random_seed();
    log.push("sol-init", "init", "syncing and unmounting");
    log.flush();
    rustix::fs::sync();
    // Anything left over (orphans outside units): TERM, a moment, KILL.
    for sig in [rustix::process::Signal::TERM, rustix::process::Signal::KILL] {
        for pid in other_pids() {
            let _ = rustix::process::kill_process(pid, sig);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        while matches!(
            // Any child, whatever its process group (see Runtime::reap).
            rustix::process::wait(rustix::process::WaitOptions::NOHANG),
            Ok(Some(_))
        ) {}
    }
    for m in ["/Users", "/var", "/etc", "/boot", BTRFS_TOP, "/tmp"] {
        let _ = unmount(m, UnmountFlags::empty());
    }
    let _ = mount_remount("/usr", MountFlags::RDONLY, "");
    let _ = mount_remount("/", MountFlags::RDONLY, "");
    rustix::fs::sync();
    let cmd = match power {
        Power::Poweroff => rustix::system::RebootCommand::PowerOff,
        _ => rustix::system::RebootCommand::Restart,
    };
    let _ = rustix::system::reboot(cmd);
    // reboot(2) only returns on error; PID 1 must never exit.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fat_volume_ids() {
        let d = tempfile::tempdir().unwrap();
        let write = |name: &str, fs_type_at: usize, fs_type: &[u8], id_at: usize| {
            let mut bs = vec![0u8; 512];
            bs[510] = 0x55;
            bs[511] = 0xAA;
            bs[fs_type_at..fs_type_at + fs_type.len()].copy_from_slice(fs_type);
            bs[id_at..id_at + 4].copy_from_slice(&0x1234_ABCDu32.to_le_bytes());
            let p = d.path().join(name);
            fs::write(&p, bs).unwrap();
            p
        };
        let fat32 = write("fat32", 0x52, b"FAT32   ", 0x43);
        assert_eq!(fat_volume_id(&fat32).as_deref(), Some("1234-ABCD"));
        let fat16 = write("fat16", 0x36, b"FAT16   ", 0x27);
        assert_eq!(fat_volume_id(&fat16).as_deref(), Some("1234-ABCD"));
        let none = write("none", 0x36, b"NTFS", 0x27);
        assert_eq!(fat_volume_id(&none), None);
        fs::write(d.path().join("short"), b"x").unwrap();
        assert_eq!(fat_volume_id(&d.path().join("short")), None);
    }

    #[test]
    fn subvolume_data_holds_only_btrfs_options() {
        let (data, flags) = subvol_mount("@snapshots/1");
        assert_eq!(data, "subvol=@snapshots/1,compress=zstd:3");
        assert!(flags.contains(MountFlags::NOATIME));
        // VFS-level words belong in the flags; btrfs would reject them.
        for word in [
            "noatime", "relatime", "ro", "rw", "nosuid", "nodev", "noexec",
        ] {
            assert!(!data.split(',').any(|o| o == word), "{word} in mount data");
        }
    }

    #[test]
    fn cmdline() {
        let c = Cmdline::parse(
            "BOOT_IMAGE=/sol/a/vmlinuz quiet sol.root=UUID=abcd-1 sol.system=@snapshots/42 sol.esp=UUID=12AB-34CD sol.verbose",
        );
        assert_eq!(c.root.as_deref(), Some("UUID=abcd-1"));
        assert_eq!(c.system.as_deref(), Some("@snapshots/42"));
        assert_eq!(c.esp.as_deref(), Some("UUID=12AB-34CD"));
        assert!(c.verbose);
        assert_eq!(Cmdline::parse("quiet"), Cmdline::default());
    }

    #[test]
    fn sysctl_parsing() {
        let v = parse_sysctl(
            "# comment\n; also\nkernel.sysrq = 0\nnet.ipv4.ip_forward=1\n-vm.swappiness = 10\nbad\n../../etc = x\n",
        );
        assert_eq!(
            v,
            [
                ("/proc/sys/kernel/sysrq".into(), "0".into()),
                ("/proc/sys/net/ipv4/ip_forward".into(), "1".into()),
                ("/proc/sys/vm/swappiness".into(), "10".into()),
            ]
        );
    }

    #[test]
    fn fsid_reading() {
        let d = tempfile::tempdir().unwrap();
        let img = d.path().join("img");
        let mut data = vec![0u8; 0x10000 + 0x1000];
        data[0x10040..0x10048].copy_from_slice(b"_BHRfS_M");
        for (i, b) in data[0x10020..0x10030].iter_mut().enumerate() {
            *b = u8::try_from(i).unwrap();
        }
        fs::write(&img, &data).unwrap();
        assert_eq!(
            btrfs_fsid(&img).as_deref(),
            Some("00010203-0405-0607-0809-0a0b0c0d0e0f")
        );
        data[0x10040] = b'x';
        fs::write(&img, &data).unwrap();
        assert_eq!(btrfs_fsid(&img), None);
    }

    #[test]
    fn etc_seeding() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("usr/share/factory/etc");
        fs::create_dir_all(f.join("sol/units")).unwrap();
        fs::write(f.join("hostname"), "solace\n").unwrap();
        fs::write(f.join("sol/units/x.toml"), "x").unwrap();
        fs::create_dir_all(d.path().join("etc")).unwrap();
        fs::write(d.path().join("etc/hostname"), "mine\n").unwrap();
        let mut log = LogStore::memory_only();
        seed_etc(d.path(), &mut log);
        assert_eq!(
            fs::read_to_string(d.path().join("etc/hostname")).unwrap(),
            "mine\n"
        );
        assert!(d.path().join("etc/sol/units/x.toml").exists());
        // Only on first boot.
        fs::remove_file(d.path().join("etc/sol/units/x.toml")).unwrap();
        seed_etc(d.path(), &mut log);
        assert!(!d.path().join("etc/sol/units/x.toml").exists());
    }
}
