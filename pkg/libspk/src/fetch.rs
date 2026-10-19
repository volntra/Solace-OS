//! Downloads: repository files and recipe sources.
//!
//! Only `https://` URLs (with HTTPS-only redirects) and local paths are
//! accepted. Every download is size-limited and hashed while streaming.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, IoContext, Result};
use crate::hash;
use crate::recipe::Source;

/// Where a repository (or any fetchable tree) lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Local(PathBuf),
    Https(String),
}

impl Location {
    /// # Errors
    /// Rejects `http://` and every scheme other than `https://`/`file://`.
    pub fn parse(s: &str) -> Result<Self> {
        if let Some(rest) = s.strip_prefix("file://") {
            return Ok(Location::Local(PathBuf::from(rest)));
        }
        if s.starts_with("https://") {
            return Ok(Location::Https(s.trim_end_matches('/').to_string()));
        }
        if s.contains("://") {
            return Err(Error::invalid(format!(
                "{s}: only https:// and local paths are allowed"
            )));
        }
        Ok(Location::Local(PathBuf::from(s)))
    }

    /// Fetches `<self>/<rel>` into `dest`, at most `max` bytes.
    ///
    /// # Errors
    /// Network, I/O, and size-limit errors.
    pub fn fetch(&self, rel: &str, dest: &Path, max: u64) -> Result<([u8; 32], u64)> {
        match self {
            Location::Local(base) => {
                let src = base.join(rel);
                let f = File::open(&src).at(&src)?;
                copy_limited(f, dest, max, &src.display().to_string())
            }
            Location::Https(base) => download(&format!("{base}/{rel}"), dest, max),
        }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Location::Local(p) => write!(f, "{}", p.display()),
            Location::Https(u) => f.write_str(u),
        }
    }
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .https_only(true)
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(30)))
        .max_redirects(10)
        .build()
        .into()
}

/// Streams `r` into `dest` (via a temp file + rename), hashing as it goes.
fn copy_limited(r: impl Read, dest: &Path, max: u64, what: &str) -> Result<([u8; 32], u64)> {
    let dir = dest.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).at(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).at(dir)?;
    let mut limited = r.take(max + 1);
    let (digest, n) = {
        let mut w = HashTee {
            inner: tmp.as_file_mut(),
            h: <sha2::Sha256 as sha2::Digest>::new(),
            n: 0,
        };
        io::copy(&mut limited, &mut w).at(dest)?;
        (sha2::Digest::finalize(w.h).into(), w.n)
    };
    if n > max {
        return Err(Error::Fetch {
            url: what.to_string(),
            reason: format!("larger than the expected {max} bytes"),
        });
    }
    tmp.as_file().sync_all().at(dest)?;
    tmp.persist(dest).map_err(|e| e.error).at(dest)?;
    Ok((digest, n))
}

struct HashTee<'a> {
    inner: &'a mut File,
    h: sha2::Sha256,
    n: u64,
}

impl Write for HashTee<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let k = self.inner.write(buf)?;
        sha2::Digest::update(&mut self.h, &buf[..k]);
        self.n += k as u64;
        Ok(k)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Downloads an `https://` URL into `dest`.
///
/// # Errors
/// [`Error::Fetch`] for network/HTTP errors and oversize bodies.
pub fn download(url: &str, dest: &Path, max: u64) -> Result<([u8; 32], u64)> {
    if !url.starts_with("https://") {
        return Err(Error::Fetch {
            url: url.into(),
            reason: "only https:// is allowed".into(),
        });
    }
    let fail = |reason: String| Error::Fetch {
        url: url.into(),
        reason,
    };
    let resp = agent().get(url).call().map_err(|e| fail(e.to_string()))?;
    let reader = resp.into_body().into_reader();
    copy_limited(reader, dest, max, url).map_err(|e| match e {
        Error::Io { source, .. } => fail(source.to_string()),
        other => other,
    })
}

/// Largest source tarball we accept (the kernel is ~150 MB).
pub const MAX_SOURCE_BYTES: u64 = 2 << 30;

/// Returns a verified copy of `src` from the content-addressed cache
/// (`<cache>/<sha256>`), downloading it from its URL or mirrors if needed.
///
/// # Errors
/// [`Error::Fetch`] if no URL yields a file with the recipe's sha256.
pub fn fetch_source(src: &Source, cache: &Path) -> Result<PathBuf> {
    fs::create_dir_all(cache).at(cache)?;
    let cached = cache.join(&src.sha256);
    if cached.is_file() {
        let (d, _) = hash::sha256_file(&cached)?;
        if hex::encode(d) == src.sha256 {
            return Ok(cached);
        }
        // Corrupt cache entry: drop it and refetch.
        fs::remove_file(&cached).at(&cached)?;
    }
    let mut errors = Vec::new();
    for url in std::iter::once(&src.url).chain(&src.mirrors) {
        let part = cache.join(format!("{}.part", src.sha256));
        match download(url, &part, MAX_SOURCE_BYTES) {
            Ok((d, _)) if hex::encode(d) == src.sha256 => {
                fs::rename(&part, &cached).at(&cached)?;
                return Ok(cached);
            }
            Ok((d, _)) => {
                let _ = fs::remove_file(&part);
                errors.push(format!(
                    "{url}: sha256 mismatch (got {}, recipe says {})",
                    hex::encode(d),
                    src.sha256
                ));
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                errors.push(e.to_string());
            }
        }
    }
    Err(Error::Fetch {
        url: src.url.clone(),
        reason: errors.join("; "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations() {
        assert_eq!(
            Location::parse("https://r.example/x/").unwrap(),
            Location::Https("https://r.example/x".into())
        );
        assert_eq!(
            Location::parse("/srv/repo").unwrap(),
            Location::Local("/srv/repo".into())
        );
        assert_eq!(
            Location::parse("file:///srv/repo").unwrap(),
            Location::Local("/srv/repo".into())
        );
        for bad in ["http://r.example", "ftp://x", "rsync://x"] {
            assert!(Location::parse(bad).is_err(), "{bad}");
        }
        assert!(download("http://example.org/x", Path::new("/nonexistent"), 1).is_err());
    }

    #[test]
    fn local_fetch_is_limited_and_hashed() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("f"), b"abc").unwrap();
        let loc = Location::Local(d.path().into());
        let (h, n) = loc.fetch("f", &d.path().join("out/g"), 3).unwrap();
        assert_eq!((hex::encode(h), n), (hash::sha256_hex(b"abc"), 3));
        assert!(loc.fetch("f", &d.path().join("out/h"), 2).is_err());
        assert!(!d.path().join("out/h").exists());
    }

    #[test]
    fn cached_source_is_reverified() {
        let d = tempfile::tempdir().unwrap();
        let sha = hash::sha256_hex(b"source");
        fs::write(d.path().join(&sha), b"source").unwrap();
        let src = Source {
            url: "https://invalid.invalid/x.tar.gz".into(),
            sha256: sha.clone(),
            mirrors: vec![],
            extract: true,
            dir: None,
            local: None,
        };
        assert_eq!(fetch_source(&src, d.path()).unwrap(), d.path().join(&sha));

        // A corrupted cache entry is discarded, and the (unreachable) URL
        // is tried instead of trusting it.
        fs::write(d.path().join(&sha), b"evil").unwrap();
        assert!(fetch_source(&src, d.path()).is_err());
        assert!(!d.path().join(&sha).exists());
    }
}
