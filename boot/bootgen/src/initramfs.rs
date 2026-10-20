//! The initramfs: a "newc" cpio archive holding only fern-init (as `/init`),
//! the mount points it uses, and `/dev/console`. fern-init finds the root
//! filesystem, assembles it from btrfs subvolumes, and switches to it
//! (init/src/pid1.rs). No shell, no busybox.

use std::io::{self, Write};

const MAGIC: &str = "070701";
/// Empty directories fern-init mounts things on.
pub const DIRS: &[&str] = &["dev", "proc", "sys", "run", "tmp", "sysroot"];

/// Linux mode bits.
const S_IFDIR: u32 = 0o040_000;
const S_IFREG: u32 = 0o100_000;
const S_IFCHR: u32 = 0o020_000;

struct Writer<W> {
    out: W,
    ino: u32,
    written: usize,
}

impl<W: Write> Writer<W> {
    fn pad(&mut self) -> io::Result<()> {
        let n = (4 - self.written % 4) % 4;
        self.out.write_all(&[0u8; 3][..n])?;
        self.written += n;
        Ok(())
    }

    fn entry(&mut self, name: &str, mode: u32, rdev: (u32, u32), data: &[u8]) -> io::Result<()> {
        self.ino += 1;
        let size = u32::try_from(data.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "file too large"))?;
        let namesize = u32::try_from(name.len() + 1)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name too long"))?;
        let nlink = if mode & S_IFDIR != 0 { 2 } else { 1 };
        let header = format!(
            "{MAGIC}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}",
            self.ino, mode, 0, 0, nlink, 0, size, 0, 0, rdev.0, rdev.1, namesize, 0
        );
        self.out.write_all(header.as_bytes())?;
        self.out.write_all(name.as_bytes())?;
        self.out.write_all(&[0])?;
        self.written += header.len() + name.len() + 1;
        self.pad()?;
        self.out.write_all(data)?;
        self.written += data.len();
        self.pad()
    }
}

/// Writes the initramfs with `init` (the fern-init binary) as `/init`.
///
/// # Errors
/// Write errors.
pub fn write(out: impl Write, init: &[u8]) -> io::Result<()> {
    let mut w = Writer {
        out,
        ino: 0,
        written: 0,
    };
    for d in DIRS {
        w.entry(d, S_IFDIR | 0o755, (0, 0), &[])?;
    }
    w.entry("dev/console", S_IFCHR | 0o600, (5, 1), &[])?;
    w.entry("init", S_IFREG | 0o755, (0, 0), init)?;
    w.entry("TRAILER!!!", 0, (0, 0), &[])?;
    w.out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (name, mode, rdev, data)
    type Entry = (String, u32, (u32, u32), Vec<u8>);

    /// Minimal newc reader for the tests.
    fn read(mut b: &[u8]) -> Vec<Entry> {
        let mut out = Vec::new();
        let total = b.len();
        loop {
            assert_eq!(&b[..6], MAGIC.as_bytes());
            let field = |i: usize| {
                u32::from_str_radix(std::str::from_utf8(&b[6 + i * 8..14 + i * 8]).unwrap(), 16)
                    .unwrap()
            };
            let (mode, size, rmaj, rmin, namesize) = (
                field(1),
                field(6) as usize,
                field(9),
                field(10),
                field(11) as usize,
            );
            let name = std::str::from_utf8(&b[110..110 + namesize - 1])
                .unwrap()
                .to_string();
            let mut off = 110 + namesize;
            off += (4 - (total - b.len() + off) % 4) % 4;
            let data = b[off..off + size].to_vec();
            off += size;
            off += (4 - (total - b.len() + off) % 4) % 4;
            b = &b[off..];
            if name == "TRAILER!!!" {
                assert!(b.is_empty());
                return out;
            }
            out.push((name, mode, (rmaj, rmin), data));
        }
    }

    #[test]
    fn layout() {
        let mut buf = Vec::new();
        write(&mut buf, b"\x7fELF fake init").unwrap();
        assert_eq!(buf.len() % 4, 0);
        let entries = read(&buf);
        let names: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(
            names,
            [
                "dev",
                "proc",
                "sys",
                "run",
                "tmp",
                "sysroot",
                "dev/console",
                "init"
            ]
        );
        let console = &entries[6];
        assert_eq!(console.1, S_IFCHR | 0o600);
        assert_eq!(console.2, (5, 1));
        let init = &entries[7];
        assert_eq!(init.1, S_IFREG | 0o755);
        assert_eq!(init.3, b"\x7fELF fake init");
    }

    #[test]
    fn odd_sizes_stay_aligned() {
        for n in 0..9 {
            let mut buf = Vec::new();
            write(&mut buf, &vec![7u8; n]).unwrap();
            assert_eq!(read(&buf)[7].3.len(), n);
        }
    }
}
