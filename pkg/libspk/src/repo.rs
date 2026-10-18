//! Signed package repositories (docs/formats/repo.md).
//!
//! Security-critical: the index signature is checked over the exact bytes
//! fetched, before the index is parsed.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fetch::Location;
use crate::hash;
use crate::manifest::{ARCH, is_sha256_hex, package_file_name};
use crate::package::{Expected, VerifiedPackage};
use crate::sign::{Keyring, Purpose, SecretKey, sig_path};
use crate::solve::Candidate;
use crate::version::{DepSpec, PkgVersion, Version, validate_name};

pub const INDEX: &str = "index.toml";
pub const INDEX_FORMAT: u32 = 1;
const MAX_INDEX_BYTES: u64 = 64 << 20;
const MAX_SIG_BYTES: u64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub name: String,
    pub version: Version,
    pub release: u32,
    pub arch: String,
    pub summary: String,
    #[serde(default)]
    pub depends: Vec<DepSpec>,
    #[serde(default)]
    pub provides: Vec<DepSpec>,
    #[serde(default)]
    pub conflicts: Vec<DepSpec>,
    #[serde(default)]
    pub replaces: Vec<DepSpec>,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    pub installed_size: u64,
}

impl IndexEntry {
    pub fn pkg_version(&self) -> PkgVersion {
        PkgVersion::new(self.version.clone(), self.release)
    }

    pub fn expected(&self) -> Expected {
        Expected {
            sha256: self.sha256.clone(),
            size: self.size,
            name: self.name.clone(),
            version: self.pkg_version(),
        }
    }

    pub fn candidate(&self) -> Candidate {
        Candidate {
            name: self.name.clone(),
            version: self.pkg_version(),
            depends: self.depends.clone(),
            provides: self.provides.clone(),
            conflicts: self.conflicts.clone(),
            replaces: self.replaces.clone(),
            installed: false,
        }
    }

    fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        if self.arch != ARCH {
            return Err(Error::invalid(format!(
                "{}: unsupported arch {}",
                self.name, self.arch
            )));
        }
        let want = format!(
            "packages/{}",
            package_file_name(&self.name, &self.pkg_version(), &self.arch)
        );
        if self.file != want {
            return Err(Error::invalid(format!(
                "{}: file must be {want}, not {}",
                self.name, self.file
            )));
        }
        if !is_sha256_hex(&self.sha256) {
            return Err(Error::invalid(format!("{}: bad sha256", self.name)));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub format: u32,
    pub name: String,
    pub generated: u64,
    #[serde(default, rename = "package")]
    pub packages: Vec<IndexEntry>,
}

impl Index {
    /// Parses and validates index text. Call only on verified bytes.
    ///
    /// # Errors
    /// [`Error::Invalid`] for malformed indexes.
    pub fn parse(text: &str) -> Result<Self> {
        let idx: Index =
            toml::from_str(text).map_err(|e| Error::invalid(format!("repository index: {e}")))?;
        if idx.format != INDEX_FORMAT {
            return Err(Error::invalid(format!(
                "repository index format {} is not supported",
                idx.format
            )));
        }
        let mut seen = std::collections::HashSet::new();
        for e in &idx.packages {
            e.validate()?;
            if !seen.insert((e.name.clone(), e.pkg_version())) {
                return Err(Error::invalid(format!(
                    "repository index lists {} {} twice",
                    e.name,
                    e.pkg_version()
                )));
            }
        }
        Ok(idx)
    }
}

/// A repository whose index has been fetched and verified.
#[derive(Debug)]
pub struct Repo {
    pub location: Location,
    pub index: Index,
}

impl Repo {
    /// Fetches `index.toml` + `.sig` into `work`, verifies the signature with
    /// `keyring`, and only then parses it.
    ///
    /// # Errors
    /// [`Error::Signature`] for an unsigned or badly signed index; fetch and
    /// parse errors otherwise.
    pub fn open(location: Location, keyring: &Keyring, work: &Path) -> Result<Self> {
        let idx_path = work.join(format!(
            "index-{}.toml",
            hash::sha256_hex(location.to_string().as_bytes())
        ));
        let sig_file = sig_path(&idx_path);
        let (digest, _) = location.fetch(INDEX, &idx_path, MAX_INDEX_BYTES)?;
        location
            .fetch(&format!("{INDEX}.sig"), &sig_file, MAX_SIG_BYTES)
            .map_err(|e| Error::Signature(format!("{location}: index is not signed ({e})")))?;
        let sig_text = fs::read_to_string(&sig_file).at(&sig_file)?;
        keyring
            .verify(Purpose::Index, &digest, &sig_text)
            .map_err(|e| Error::Signature(format!("{location}: index: {e}")))?;
        // Parse the bytes that were verified (the file is private to `work`).
        let bytes = fs::read(&idx_path).at(&idx_path)?;
        if hash::sha256(&bytes) != digest {
            return Err(Error::Verify(format!(
                "{location}: index changed while reading"
            )));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| Error::invalid(format!("{location}: index is not UTF-8")))?;
        let index = Index::parse(&text)?;
        Ok(Repo { location, index })
    }

    /// Downloads a package and verifies it against its index entry and its
    /// own signature.
    ///
    /// # Errors
    /// Fetch, signature, and verification errors.
    pub fn fetch_package(
        &self,
        entry: &IndexEntry,
        keyring: &Keyring,
        work: &Path,
    ) -> Result<VerifiedPackage> {
        let local = work.join(entry.file.rsplit('/').next().unwrap_or("package.solpkg"));
        self.location.fetch(&entry.file, &local, entry.size)?;
        self.location
            .fetch(
                &format!("{}.sig", entry.file),
                &sig_path(&local),
                MAX_SIG_BYTES,
            )
            .map_err(|e| Error::Signature(format!("{}: no signature ({e})", entry.file)))?;
        VerifiedPackage::open(&local, keyring, Some(&entry.expected()), work)
    }
}

/// Adds packages to the repository at `dir`, replacing older entries with
/// the same name, and re-signs the index with `key`.
///
/// Every package (and the existing index, if any) must verify against
/// `keyring` first: `repo-add` never launders an unsigned file into a
/// signed index.
///
/// # Errors
/// Verification and I/O errors. The repository is unchanged on error,
/// except for package files copied before the failure.
pub fn repo_add(
    dir: &Path,
    name: &str,
    packages: &[PathBuf],
    key: &SecretKey,
    keyring: &Keyring,
) -> Result<Index> {
    let work = tempfile::tempdir().at(std::env::temp_dir())?;
    let pkg_dir = dir.join("packages");
    fs::create_dir_all(&pkg_dir).at(&pkg_dir)?;

    let mut index = if dir.join(INDEX).exists() {
        Repo::open(Location::Local(dir.to_path_buf()), keyring, work.path())?.index
    } else {
        Index {
            format: INDEX_FORMAT,
            name: name.to_string(),
            generated: 0,
            packages: Vec::new(),
        }
    };

    for p in packages {
        let vp = VerifiedPackage::open(p, keyring, None, work.path())?;
        let m = &vp.manifest;
        let file_name = m.file_name();
        let dest = pkg_dir.join(&file_name);
        if dest != *p {
            fs::copy(p, &dest).at(&dest)?;
            fs::copy(sig_path(p), sig_path(&dest)).at(sig_path(&dest))?;
        }
        // Drop older versions of this package from the index and disk.
        for old in index.packages.iter().filter(|e| e.name == m.name) {
            let old_path = dir.join(&old.file);
            if old_path != dest {
                let _ = fs::remove_file(&old_path);
                let _ = fs::remove_file(sig_path(&old_path));
            }
        }
        index.packages.retain(|e| e.name != m.name);
        index.packages.push(IndexEntry {
            name: m.name.clone(),
            version: m.version.clone(),
            release: m.release,
            arch: m.arch.clone(),
            summary: m.summary.clone(),
            depends: m.depends.clone(),
            provides: m.provides.clone(),
            conflicts: m.conflicts.clone(),
            replaces: m.replaces.clone(),
            file: format!("packages/{file_name}"),
            size: vp.size,
            sha256: vp.sha256.clone(),
            installed_size: m.installed_size,
        });
    }
    index.packages.sort_by(|a, b| a.name.cmp(&b.name));
    index.generated = u64::try_from(crate::db::now()).unwrap_or(0);

    let text = toml::to_string(&index).map_err(|e| Error::invalid(e.to_string()))?;
    let tmp = dir.join(format!(".{INDEX}.new"));
    fs::write(&tmp, &text).at(&tmp)?;
    let digest = hash::sha256(text.as_bytes());
    let sig = key.sign(Purpose::Index, &digest).to_text();
    let tmp_sig = dir.join(format!(".{INDEX}.sig.new"));
    fs::write(&tmp_sig, sig).at(&tmp_sig)?;
    // Signature first: a reader never sees a new index with an old sig
    // that verifies (it would fail closed either way).
    fs::rename(&tmp_sig, dir.join(format!("{INDEX}.sig"))).at(dir)?;
    fs::rename(&tmp, dir.join(INDEX)).at(dir)?;
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::tests::{Fixture, build_pkg, meta};

    fn repo_with(fx: &Fixture, pkgs: &[PathBuf]) -> PathBuf {
        let dir = fx.path("repo");
        repo_add(&dir, "test", pkgs, &fx.key, &fx.ring).unwrap();
        dir
    }

    #[test]
    fn add_open_fetch() {
        let fx = Fixture::new();
        let a = build_pkg(&fx, &meta("hello", "1.0", &[]), "A");
        let dir = repo_with(&fx, &[a]);
        let repo = Repo::open(Location::Local(dir.clone()), &fx.ring, &fx.work()).unwrap();
        assert_eq!(repo.index.packages.len(), 1);
        let e = &repo.index.packages[0];
        assert_eq!(e.file, "packages/hello-1.0-1.x86_64.solpkg");
        let vp = repo.fetch_package(e, &fx.ring, &fx.work()).unwrap();
        assert_eq!(vp.manifest.name, "hello");
    }

    #[test]
    fn newer_version_replaces_entry() {
        let fx = Fixture::new();
        let a = build_pkg(&fx, &meta("hello", "1.0", &[]), "A");
        let dir = repo_with(&fx, &[a]);
        let b = build_pkg(&fx, &meta("hello", "1.1", &[]), "B");
        let idx = repo_add(&dir, "test", &[b], &fx.key, &fx.ring).unwrap();
        assert_eq!(idx.packages.len(), 1);
        assert_eq!(idx.packages[0].version.as_str(), "1.1");
        assert!(!dir.join("packages/hello-1.0-1.x86_64.solpkg").exists());
    }

    #[test]
    fn tampered_index_refused() {
        let fx = Fixture::new();
        let a = build_pkg(&fx, &meta("hello", "1.0", &[]), "A");
        let dir = repo_with(&fx, &[a]);
        let text = fs::read_to_string(dir.join(INDEX)).unwrap();
        fs::write(dir.join(INDEX), text.replace("hello test package", "evil")).unwrap();
        let e = Repo::open(Location::Local(dir), &fx.ring, &fx.work()).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");
    }

    #[test]
    fn unsigned_index_refused() {
        let fx = Fixture::new();
        let a = build_pkg(&fx, &meta("hello", "1.0", &[]), "A");
        let dir = repo_with(&fx, &[a]);
        fs::remove_file(dir.join("index.toml.sig")).unwrap();
        let e = Repo::open(Location::Local(dir), &fx.ring, &fx.work()).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");
    }

    #[test]
    fn swapped_package_refused_even_if_signed() {
        // An attacker swaps in an *older, validly signed* package under the
        // indexed name: the index hash catches it.
        let fx = Fixture::new();
        let old = build_pkg(&fx, &meta("hello", "1.0", &[]), "OLD");
        let old_bytes = fs::read(&old).unwrap();
        let old_sig = fs::read(sig_path(&old)).unwrap();
        fs::remove_file(&old).unwrap();
        let new = build_pkg(&fx, &meta("hello", "1.0", &[]), "NEW");
        let dir = repo_with(&fx, &[new]);
        let target = dir.join("packages/hello-1.0-1.x86_64.solpkg");
        fs::write(&target, old_bytes).unwrap();
        fs::write(sig_path(&target), old_sig).unwrap();
        let repo = Repo::open(Location::Local(dir), &fx.ring, &fx.work()).unwrap();
        let e = repo
            .fetch_package(&repo.index.packages[0], &fx.ring, &fx.work())
            .unwrap_err();
        assert!(matches!(e, Error::Verify(_)), "{e}");
    }

    #[test]
    fn repo_add_refuses_untrusted_packages() {
        let fx = Fixture::new();
        let a = build_pkg(&fx, &meta("hello", "1.0", &[]), "A");
        let other = SecretKey::generate().unwrap();
        other.sign_file(Purpose::Package, &a).unwrap();
        assert!(repo_add(&fx.path("repo"), "t", &[a], &fx.key, &fx.ring).is_err());
    }

    #[test]
    fn index_validation() {
        let fx = Fixture::new();
        let a = build_pkg(&fx, &meta("hello", "1.0", &[]), "A");
        let dir = repo_with(&fx, &[a]);
        let text = fs::read_to_string(dir.join(INDEX)).unwrap();
        let bad = text.replace("packages/hello-1.0-1", "packages/../../hello-1.0-1");
        assert!(Index::parse(&bad).is_err());
        Index::parse(&text).unwrap();
    }
}
