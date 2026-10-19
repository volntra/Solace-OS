//! The few btrfs operations system transactions need: snapshot a
//! subvolume, make one read-only (seal it), ask whether it is, and delete
//! one. Thin wrappers over the kernel's ioctls (linux/btrfs.h); the
//! numbers and the argument layout are pinned by the tests below.

use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::error::{Error, IoContext, Result};

const MAGIC: u64 = 0x94;
/// `BTRFS_SUBVOL_NAME_MAX + 1`.
const NAME_LEN: usize = 4040;
/// `BTRFS_SUBVOL_RDONLY`.
const SUBVOL_RDONLY: u64 = 1 << 1;
/// The inode number of every subvolume's root directory.
const SUBVOL_ROOT_INO: u64 = 256;

/// `struct btrfs_ioctl_vol_args_v2` (4096 bytes).
#[repr(C)]
struct VolArgsV2 {
    fd: i64,
    transid: u64,
    flags: u64,
    unused: [u64; 4],
    name: [u8; NAME_LEN],
}

const fn ioc(dir: u64, nr: u64, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | (MAGIC << 8) | nr
}
const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;
const SNAP_CREATE_V2: u64 = ioc(IOC_WRITE, 23, size_of::<VolArgsV2>());
const SNAP_DESTROY_V2: u64 = ioc(IOC_WRITE, 63, size_of::<VolArgsV2>());
const SUBVOL_GETFLAGS: u64 = ioc(IOC_READ, 25, size_of::<u64>());
const SUBVOL_SETFLAGS: u64 = ioc(IOC_WRITE, 26, size_of::<u64>());

fn vol_args(name: &str, fd: i64, flags: u64) -> Result<Box<VolArgsV2>> {
    let valid = !name.is_empty()
        && name.len() < NAME_LEN
        && name != "."
        && name != ".."
        && !name.bytes().any(|b| b == b'/' || b == 0);
    if !valid {
        return Err(Error::invalid(format!("bad subvolume name {name:?}")));
    }
    let mut a = Box::new(VolArgsV2 {
        fd,
        transid: 0,
        flags,
        unused: [0; 4],
        name: [0; NAME_LEN],
    });
    a.name[..name.len()].copy_from_slice(name.as_bytes());
    Ok(a)
}

/// One ioctl on `fd` with a pointer argument.
fn ioctl<T>(file: &File, request: u64, arg: *mut T, what: &Path) -> Result<()> {
    // SAFETY: `file` is an open descriptor for the call's duration, and
    // `arg` points to a live, correctly sized `#[repr(C)]` value matching
    // the kernel's definition for `request` (pinned by the tests below).
    #[allow(unsafe_code)]
    let r = unsafe { libc::ioctl(file.as_raw_fd(), request as libc::Ioctl, arg) };
    if r < 0 {
        return Err(std::io::Error::last_os_error()).at(what);
    }
    Ok(())
}

fn open_dir(p: &Path) -> Result<File> {
    File::open(p).at(p)
}

/// Whether `p` is the root of a btrfs subvolume.
pub fn is_subvolume(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && m.ino() == SUBVOL_ROOT_INO)
}

/// Snapshots subvolume `src` as `<parent>/<name>` (writable unless
/// `read_only`).
///
/// # Errors
/// I/O errors from the kernel (an existing name, not a subvolume, ...).
pub fn snapshot(src: &Path, parent: &Path, name: &str, read_only: bool) -> Result<()> {
    let s = open_dir(src)?;
    let dir = open_dir(parent)?;
    let flags = if read_only { SUBVOL_RDONLY } else { 0 };
    let mut a = vol_args(name, i64::from(s.as_raw_fd()), flags)?;
    ioctl(&dir, SNAP_CREATE_V2, &raw mut *a, &parent.join(name))
}

/// Whether subvolume `p` is read-only (sealed).
///
/// # Errors
/// I/O errors, or `p` not being a subvolume.
pub fn is_read_only(p: &Path) -> Result<bool> {
    let f = open_dir(p)?;
    let mut flags: u64 = 0;
    ioctl(&f, SUBVOL_GETFLAGS, &raw mut flags, p)?;
    Ok(flags & SUBVOL_RDONLY != 0)
}

/// Makes subvolume `p` read-only. Its files must be synced first: the
/// kernel refuses writes afterwards, but cached ones still drain.
///
/// # Errors
/// I/O errors.
pub fn seal(p: &Path) -> Result<()> {
    let f = open_dir(p)?;
    let mut flags: u64 = 0;
    ioctl(&f, SUBVOL_GETFLAGS, &raw mut flags, p)?;
    flags |= SUBVOL_RDONLY;
    ioctl(&f, SUBVOL_SETFLAGS, &raw mut flags, p)
}

/// Deletes subvolume `<parent>/<name>` (read-only or not).
///
/// # Errors
/// I/O errors (EBUSY while it is mounted).
pub fn delete(parent: &Path, name: &str) -> Result<()> {
    let dir = open_dir(parent)?;
    let mut a = vol_args(name, 0, 0)?;
    ioctl(&dir, SNAP_DESTROY_V2, &raw mut *a, &parent.join(name))
}

/// `syncfs(2)` on the filesystem holding `p`.
///
/// # Errors
/// I/O errors.
pub fn sync_fs(p: &Path) -> Result<()> {
    let f = open_dir(p)?;
    // SAFETY: plain syscall on a descriptor that is open for the call.
    #[allow(unsafe_code)]
    let r = unsafe { libc::syncfs(f.as_raw_fd()) };
    if r < 0 {
        return Err(std::io::Error::last_os_error()).at(p);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_abi() {
        // Values from linux/btrfs.h as built (btrfs-progs agrees).
        assert_eq!(size_of::<VolArgsV2>(), 4096);
        assert_eq!(std::mem::offset_of!(VolArgsV2, name), 56);
        assert_eq!(SNAP_CREATE_V2, 0x5000_9417);
        assert_eq!(SNAP_DESTROY_V2, 0x5000_943f);
        assert_eq!(SUBVOL_GETFLAGS, 0x8008_9419);
        assert_eq!(SUBVOL_SETFLAGS, 0x4008_941a);
    }

    #[test]
    fn subvolume_names_are_single_components() {
        assert!(vol_args("2", 0, 0).is_ok());
        for bad in ["", ".", "..", "a/b", "a\0b", &"x".repeat(NAME_LEN)] {
            assert!(vol_args(bad, 0, 0).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_plain_directory_is_not_a_subvolume() {
        let d = tempfile::tempdir().unwrap();
        assert!(!is_subvolume(d.path()));
        assert!(!is_subvolume(&d.path().join("missing")));
    }
}
