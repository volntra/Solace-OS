use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{IoContext, Result};

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256(data))
}

/// Streams a reader through sha256, returning (digest, byte count).
///
/// # Errors
/// Propagates read errors.
pub fn sha256_reader(mut r: impl Read) -> io::Result<([u8; 32], u64)> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut n = 0u64;
    loop {
        let k = r.read(&mut buf)?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((h.finalize().into(), n))
}

/// # Errors
/// I/O errors opening or reading `path`.
pub fn sha256_file(path: &Path) -> Result<([u8; 32], u64)> {
    let f = File::open(path).at(path)?;
    sha256_reader(f).at(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_vector() {
        assert_eq!(
            super::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let (d, n) = super::sha256_reader(&b"abc"[..]).unwrap();
        assert_eq!(hex::encode(d), super::sha256_hex(b"abc"));
        assert_eq!(n, 3);
    }
}
