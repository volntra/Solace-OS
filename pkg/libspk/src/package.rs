//! `.fernpkg` archives: creation, verification, and safe extraction.
//!
//! Security-critical. Spec: docs/formats/package.md. The verification
//! order there is implemented by [`VerifiedPackage::open`]; extraction by
//! [`VerifiedPackage::extract`].

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::{Error, IoContext, Result};
use crate::hash;
use crate::manifest::{ARCH, FORMAT, FileEntry, FileKind, Manifest, validate_payload_path};
use crate::sign::{Keyring, Purpose, SecretKey, sig_path};
use crate::version::{DepSpec, Version};

pub const MANIFEST_NAME: &str = ".MANIFEST";
const MAX_MANIFEST_BYTES: u64 = 16 << 20;
const ZSTD_LEVEL: i32 = 12;

/// Everything about a package except its file list.
#[derive(Clone, Debug)]
pub struct PackageMeta {
    pub name: String,
    pub version: Version,
    pub release: u32,
    pub summary: String,
    pub license: String,
    pub depends: Vec<DepSpec>,
    pub provides: Vec<DepSpec>,
    pub conflicts: Vec<DepSpec>,
    pub replaces: Vec<DepSpec>,
    pub build_date: u64,
    pub recipe_sha256: String,
}

/// Describes `paths` (relative to `src`, directories included) without
/// following symlinks. Hard-linked files become independent files.
///
/// # Errors
/// Unsupported file types and invalid paths.
pub fn describe_files(src: &Path, paths: &[String]) -> Result<Vec<FileEntry>> {
    let mut out = Vec::with_capacity(paths.len());
    for rel in paths {
        validate_payload_path(rel)?;
        let full = src.join(rel);
        let meta = fs::symlink_metadata(&full).at(&full)?;
        let ft = meta.file_type();
        let mode = meta.mode() & 0o7777;
        let entry = if ft.is_symlink() {
            let target = fs::read_link(&full).at(&full)?;
            let target = target
                .to_str()
                .ok_or_else(|| Error::invalid(format!("{rel}: symlink target is not UTF-8")))?;
            FileEntry {
                path: rel.clone(),
                kind: FileKind::Symlink,
                mode: None,
                size: None,
                sha256: None,
                target: Some(target.to_string()),
            }
        } else if ft.is_dir() {
            FileEntry {
                path: rel.clone(),
                kind: FileKind::Dir,
                mode: Some(mode),
                size: None,
                sha256: None,
                target: None,
            }
        } else if ft.is_file() {
            let f = open_nofollow(&full)?;
            let (digest, size) = hash::sha256_reader(f).at(&full)?;
            FileEntry {
                path: rel.clone(),
                kind: FileKind::File,
                mode: Some(mode),
                size: Some(size),
                sha256: Some(hex::encode(digest)),
                target: None,
            }
        } else {
            return Err(Error::invalid(format!(
                "{rel}: device nodes, FIFOs, and sockets cannot be packaged"
            )));
        };
        entry.validate()?;
        out.push(entry);
    }
    Ok(out)
}

fn open_nofollow(path: &Path) -> Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .at(path)
}

fn header(kind: tar::EntryType, mode: u32, size: u64, mtime: u64) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(kind);
    h.set_mode(mode);
    h.set_size(size);
    h.set_mtime(mtime);
    h.set_uid(0);
    h.set_gid(0);
    // Only fails for names longer than the field; these fit.
    let _ = h.set_username("root");
    let _ = h.set_groupname("root");
    h
}

/// Builds and signs `<out_dir>/<name>-<ver>-<rel>.x86_64.fernpkg` from the
/// files under `src`. Returns the package path and its manifest.
///
/// # Errors
/// I/O errors and invalid file trees.
pub fn create(
    src: &Path,
    paths: &[String],
    meta: &PackageMeta,
    out_dir: &Path,
    key: &SecretKey,
) -> Result<(PathBuf, Manifest)> {
    let mut sorted: Vec<String> = paths.to_vec();
    sorted.sort();
    sorted.dedup();
    let files = describe_files(src, &sorted)?;
    let installed_size = files.iter().filter_map(|f| f.size).sum();
    let manifest = Manifest {
        format: FORMAT,
        name: meta.name.clone(),
        version: meta.version.clone(),
        release: meta.release,
        arch: ARCH.to_string(),
        summary: meta.summary.clone(),
        license: meta.license.clone(),
        depends: meta.depends.clone(),
        provides: meta.provides.clone(),
        conflicts: meta.conflicts.clone(),
        replaces: meta.replaces.clone(),
        build_date: meta.build_date,
        recipe_sha256: meta.recipe_sha256.clone(),
        installed_size,
        files,
    };
    manifest.validate()?;

    fs::create_dir_all(out_dir).at(out_dir)?;
    let final_path = out_dir.join(manifest.file_name());
    let mut tmp = tempfile::NamedTempFile::new_in(out_dir).at(out_dir)?;
    {
        let enc = zstd::Encoder::new(tmp.as_file_mut(), ZSTD_LEVEL).at(&final_path)?;
        let mut tar = tar::Builder::new(enc);
        let mtime = meta.build_date;

        let text = manifest.to_toml()?;
        let mut h = header(tar::EntryType::Regular, 0o644, text.len() as u64, mtime);
        tar.append_data(&mut h, MANIFEST_NAME, text.as_bytes())
            .at(&final_path)?;

        for f in &manifest.files {
            let full = src.join(&f.path);
            match f.kind {
                FileKind::Dir => {
                    let mut h =
                        header(tar::EntryType::Directory, f.mode.unwrap_or(0o755), 0, mtime);
                    tar.append_data(&mut h, &f.path, io::empty()).at(&full)?;
                }
                FileKind::File => {
                    let file = open_nofollow(&full)?;
                    let size = f.size.unwrap_or(0);
                    let mut h = header(
                        tar::EntryType::Regular,
                        f.mode.unwrap_or(0o644),
                        size,
                        mtime,
                    );
                    // take(): never write more than was hashed.
                    tar.append_data(&mut h, &f.path, file.take(size))
                        .at(&full)?;
                }
                FileKind::Symlink => {
                    let mut h = header(tar::EntryType::Symlink, 0o777, 0, mtime);
                    tar.append_link(&mut h, &f.path, f.target.as_deref().unwrap_or_default())
                        .at(&full)?;
                }
            }
        }
        let enc = tar.into_inner().at(&final_path)?;
        enc.finish().at(&final_path)?;
    }
    tmp.as_file().sync_all().at(&final_path)?;
    tmp.persist(&final_path)
        .map_err(|e| e.error)
        .at(&final_path)?;
    // Files created by NamedTempFile are 0600; packages are public.
    fs::set_permissions(&final_path, fs::Permissions::from_mode(0o644)).at(&final_path)?;
    key.sign_file(Purpose::Package, &final_path)?;
    Ok((final_path, manifest))
}

/// What an index says a package file must be.
#[derive(Clone, Debug)]
pub struct Expected {
    pub sha256: String,
    pub size: u64,
    pub name: String,
    pub version: crate::version::PkgVersion,
}

/// A package whose signature has been verified, held as a private copy so
/// it cannot change between verification and extraction.
#[derive(Debug)]
pub struct VerifiedPackage {
    copy: File,
    pub manifest: Manifest,
    pub sha256: String,
    pub size: u64,
    /// Where it was opened from (for messages only).
    pub origin: PathBuf,
}

impl VerifiedPackage {
    /// Verifies a package (docs/formats/package.md, "Verification").
    /// `work` must be a directory only spk can write to; the private copy
    /// is an unlinked file there.
    ///
    /// # Errors
    /// [`Error::Signature`] and [`Error::Verify`] for anything that does not
    /// check out, I/O errors otherwise.
    pub fn open(
        path: &Path,
        keyring: &Keyring,
        expected: Option<&Expected>,
        work: &Path,
    ) -> Result<Self> {
        // 0. Read the signature first; refuse early if there is none.
        let sp = sig_path(path);
        let sig_text = fs::read_to_string(&sp).map_err(|e| {
            Error::Signature(format!("{}: no readable signature ({e})", path.display()))
        })?;

        // 1. Copy into a private, unlinked temp file while hashing.
        let mut src = open_nofollow(path)?;
        let mut copy = tempfile::tempfile_in(work).at(work)?;
        let (digest, size) = {
            let mut tee = HashingWriter::new(&mut copy);
            io::copy(&mut src, &mut tee).at(path)?;
            tee.finish()
        };

        // 2. Signature over the bytes we hold.
        keyring
            .verify(Purpose::Package, &digest, &sig_text)
            .map_err(|e| e.sig_context(path.display()))?;
        let sha256 = hex::encode(digest);

        // 3. Index entry, if this came from a repo.
        if let Some(exp) = expected
            && (exp.size != size || exp.sha256 != sha256)
        {
            return Err(Error::Verify(format!(
                "{}: does not match the signed repository index",
                path.display()
            )));
        }

        // 4. Only now decompress, and only the manifest.
        copy.seek(SeekFrom::Start(0)).at(path)?;
        let manifest = read_manifest(&copy).map_err(|e| match e {
            Error::Verify(m) => Error::Verify(format!("{}: {m}", path.display())),
            other => other,
        })?;
        if let Some(exp) = expected
            && (manifest.name != exp.name || manifest.pkg_version() != exp.version)
        {
            return Err(Error::Verify(format!(
                "{}: contains {} {} but the index says {} {}",
                path.display(),
                manifest.name,
                manifest.pkg_version(),
                exp.name,
                exp.version
            )));
        }
        if let Some(fname) = path.file_name().and_then(|n| n.to_str())
            && fname != manifest.file_name()
        {
            return Err(Error::Verify(format!(
                "{}: file name does not match its manifest ({})",
                path.display(),
                manifest.file_name()
            )));
        }
        Ok(VerifiedPackage {
            copy,
            manifest,
            sha256,
            size,
            origin: path.to_path_buf(),
        })
    }

    /// Extracts the payload into `dest`, which must be an empty directory
    /// created by the caller. Every entry is checked against the manifest
    /// (docs/formats/package.md step 5).
    ///
    /// # Errors
    /// [`Error::Verify`] on any mismatch; the partially written `dest` must
    /// then be discarded by the caller.
    // Kept as one function: every check an entry passes is visible in one
    // place for review.
    #[allow(clippy::too_many_lines)]
    pub fn extract(&self, dest: &Path) -> Result<()> {
        if fs::read_dir(dest).at(dest)?.next().is_some() {
            return Err(Error::invalid(format!("{} is not empty", dest.display())));
        }
        let mut file = self.copy.try_clone().at(&self.origin)?;
        file.seek(SeekFrom::Start(0)).at(&self.origin)?;
        let dec = zstd::Decoder::new(BufReader::new(file)).at(&self.origin)?;
        let mut ar = tar::Archive::new(dec);
        let bad = |m: String| Error::Verify(format!("{}: {m}", self.origin.display()));

        let wanted: HashMap<&str, &FileEntry> = self
            .manifest
            .files
            .iter()
            .map(|f| (f.path.as_str(), f))
            .collect();
        let mut seen: HashSet<String> = HashSet::new();
        let mut made_dirs: HashSet<String> = HashSet::new();
        let mut dir_modes: Vec<(PathBuf, u32)> = Vec::new();
        let mut budget = self.manifest.installed_size;

        let mut entries = ar.entries().at(&self.origin)?;
        // The manifest entry (already parsed in open()).
        match entries.next() {
            Some(Ok(e)) if e.path().ok().as_deref() == Some(Path::new(MANIFEST_NAME)) => {}
            _ => return Err(bad("first entry is not .MANIFEST".into())),
        }
        for entry in entries {
            let mut entry = entry.map_err(|e| bad(format!("corrupt archive: {e}")))?;
            let raw_path = entry
                .path()
                .map_err(|e| bad(format!("bad entry path: {e}")))?
                .into_owned();
            let path = raw_path
                .to_str()
                .ok_or_else(|| bad("non-UTF-8 entry path".into()))?
                .to_string();
            validate_payload_path(&path).map_err(|e| bad(e.to_string()))?;
            let Some(want) = wanted.get(path.as_str()) else {
                return Err(bad(format!("{path} is not in the manifest")));
            };
            if !seen.insert(path.clone()) {
                return Err(bad(format!("{path} appears twice")));
            }
            // The parent must be a directory this extraction created, so
            // nothing is ever written through a symlink.
            if let Some((parent, _)) = path.rsplit_once('/')
                && !made_dirs.contains(parent)
            {
                return Err(bad(format!(
                    "{path}: parent {parent} was not extracted first"
                )));
            }
            let out = dest.join(&path);
            let et = entry.header().entry_type();
            let hmode = entry.header().mode().map_err(|e| bad(e.to_string()))? & 0o7777;
            match (want.kind, et) {
                (FileKind::Dir, tar::EntryType::Directory) => {
                    if Some(hmode) != want.mode {
                        return Err(bad(format!("{path}: mode differs from manifest")));
                    }
                    fs::create_dir(&out).at(&out)?;
                    made_dirs.insert(path.clone());
                    dir_modes.push((out, hmode));
                }
                (FileKind::File, tar::EntryType::Regular) => {
                    let size = entry.header().size().map_err(|e| bad(e.to_string()))?;
                    if Some(size) != want.size || Some(hmode) != want.mode {
                        return Err(bad(format!("{path}: size or mode differs from manifest")));
                    }
                    budget = budget
                        .checked_sub(size)
                        .ok_or_else(|| bad("payload larger than installed_size".into()))?;
                    let mut f = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .custom_flags(libc::O_NOFOLLOW)
                        .mode(0o600)
                        .open(&out)
                        .at(&out)?;
                    let (digest, n) = {
                        let mut tee = HashingWriter::new(&mut f);
                        io::copy(&mut (&mut entry).take(size), &mut tee).at(&out)?;
                        tee.finish()
                    };
                    if n != size || Some(hex::encode(digest)) != want.sha256 {
                        return Err(bad(format!("{path}: content does not match manifest hash")));
                    }
                    f.set_permissions(fs::Permissions::from_mode(hmode))
                        .at(&out)?;
                }
                (FileKind::Symlink, tar::EntryType::Symlink) => {
                    let target = entry
                        .link_name()
                        .map_err(|e| bad(e.to_string()))?
                        .ok_or_else(|| bad(format!("{path}: symlink without target")))?;
                    if target.to_str() != want.target.as_deref() {
                        return Err(bad(format!("{path}: symlink target differs from manifest")));
                    }
                    std::os::unix::fs::symlink(&target, &out).at(&out)?;
                }
                (k, t) => {
                    return Err(bad(format!("{path}: archive has {t:?}, manifest says {k}")));
                }
            }
        }
        if seen.len() != wanted.len() {
            let missing = wanted
                .keys()
                .find(|p| !seen.contains(**p))
                .copied()
                .unwrap_or("?");
            return Err(bad(format!(
                "{missing} is in the manifest but not the archive"
            )));
        }
        // Directory modes last (deepest first) so read-only dirs don't block
        // their own contents.
        for (p, mode) in dir_modes.iter().rev() {
            fs::set_permissions(p, fs::Permissions::from_mode(*mode)).at(p)?;
        }
        Ok(())
    }
}

/// Reads and parses `.MANIFEST` from the start of a package stream.
fn read_manifest(file: &File) -> Result<Manifest> {
    let dec = zstd::Decoder::new(BufReader::new(file))
        .map_err(|e| Error::Verify(format!("not a zstd stream: {e}")))?;
    let mut ar = tar::Archive::new(dec);
    let mut entries = ar
        .entries()
        .map_err(|e| Error::Verify(format!("corrupt archive: {e}")))?;
    let mut first = entries
        .next()
        .ok_or_else(|| Error::Verify("empty archive".into()))?
        .map_err(|e| Error::Verify(format!("corrupt archive: {e}")))?;
    let is_manifest = first.path().ok().as_deref() == Some(Path::new(MANIFEST_NAME))
        && first.header().entry_type() == tar::EntryType::Regular;
    if !is_manifest {
        return Err(Error::Verify("first entry is not .MANIFEST".into()));
    }
    let size = first.header().size().unwrap_or(u64::MAX);
    if size > MAX_MANIFEST_BYTES {
        return Err(Error::Verify("manifest is too large".into()));
    }
    let mut text = String::new();
    (&mut first)
        .take(size)
        .read_to_string(&mut text)
        .map_err(|e| Error::Verify(format!("unreadable manifest: {e}")))?;
    Manifest::parse(&text).map_err(|e| Error::Verify(e.to_string()))
}

/// A writer that hashes everything passing through it.
struct HashingWriter<W> {
    inner: W,
    hasher: sha2::Sha256,
    n: u64,
}

impl<W: Write> HashingWriter<W> {
    fn new(inner: W) -> Self {
        use sha2::Digest;
        HashingWriter {
            inner,
            hasher: sha2::Sha256::new(),
            n: 0,
        }
    }
    fn finish(self) -> ([u8; 32], u64) {
        use sha2::Digest;
        (self.hasher.finalize().into(), self.n)
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use sha2::Digest;
        let k = self.inner.write(buf)?;
        self.hasher.update(&buf[..k]);
        self.n += k as u64;
        Ok(k)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::version::PkgVersion;

    pub struct Fixture {
        pub dir: tempfile::TempDir,
        pub key: SecretKey,
        pub ring: Keyring,
    }

    impl Fixture {
        pub fn new() -> Self {
            let key = SecretKey::generate().unwrap();
            let ring = Keyring::new(vec![key.public()]);
            Fixture {
                dir: tempfile::tempdir().unwrap(),
                key,
                ring,
            }
        }
        pub fn path(&self, p: &str) -> PathBuf {
            self.dir.path().join(p)
        }
        pub fn work(&self) -> PathBuf {
            let w = self.path("work");
            fs::create_dir_all(&w).unwrap();
            w
        }
    }

    pub fn meta(name: &str, ver: &str, depends: &[&str]) -> PackageMeta {
        PackageMeta {
            name: name.into(),
            version: Version::parse(ver).unwrap(),
            release: 1,
            summary: format!("{name} test package"),
            license: "MIT".into(),
            depends: depends.iter().map(|d| DepSpec::parse(d).unwrap()).collect(),
            provides: vec![],
            conflicts: vec![],
            replaces: vec![],
            build_date: 1_700_000_000,
            recipe_sha256: "0".repeat(64),
        }
    }

    /// Builds a small tree and packages it.
    pub fn build_pkg(fx: &Fixture, m: &PackageMeta, extra_file: &str) -> PathBuf {
        let src = fx.path(&format!("src-{}-{}", m.name, m.version));
        let _ = fs::remove_dir_all(&src);
        fs::create_dir_all(src.join("usr/bin")).unwrap();
        fs::create_dir_all(src.join("usr/share/doc")).unwrap();
        fs::write(
            src.join(format!("usr/bin/{}", m.name)),
            b"#!/bin/sh\necho hi\n",
        )
        .unwrap();
        fs::set_permissions(
            src.join(format!("usr/bin/{}", m.name)),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::write(src.join(format!("usr/share/doc/{extra_file}")), b"doc").unwrap();
        std::os::unix::fs::symlink(m.name.clone(), src.join(format!("usr/bin/{}-link", m.name)))
            .unwrap();
        let paths: Vec<String> = walkdir::WalkDir::new(&src)
            .min_depth(1)
            .into_iter()
            .map(|e| {
                e.unwrap()
                    .path()
                    .strip_prefix(&src)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        let (p, _) = create(&src, &paths, m, &fx.path("out"), &fx.key).unwrap();
        p
    }

    #[test]
    fn create_open_extract_roundtrip() {
        let fx = Fixture::new();
        let p = build_pkg(&fx, &meta("hello", "1.0", &["zlib"]), "README");
        assert_eq!(p.file_name().unwrap(), "hello-1.0-1.x86_64.fernpkg");
        let vp = VerifiedPackage::open(&p, &fx.ring, None, &fx.work()).unwrap();
        assert_eq!(vp.manifest.name, "hello");
        assert_eq!(vp.manifest.files.len(), 7);
        let dest = fx.path("dest");
        fs::create_dir(&dest).unwrap();
        vp.extract(&dest).unwrap();
        assert_eq!(
            fs::read(dest.join("usr/bin/hello")).unwrap(),
            b"#!/bin/sh\necho hi\n"
        );
        let mode = fs::metadata(dest.join("usr/bin/hello"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(
            fs::read_link(dest.join("usr/bin/hello-link")).unwrap(),
            Path::new("hello")
        );
    }

    #[test]
    fn reproducible() {
        let fx = Fixture::new();
        let m = meta("hello", "1.0", &[]);
        let p = build_pkg(&fx, &m, "README");
        let first = fs::read(&p).unwrap();
        fs::remove_file(&p).unwrap();
        let src = fx.path("src-hello-1.0");
        let mut paths: Vec<String> = walkdir::WalkDir::new(&src)
            .min_depth(1)
            .into_iter()
            .map(|e| {
                e.unwrap()
                    .path()
                    .strip_prefix(&src)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .into()
            })
            .collect();
        paths.reverse();
        let (p2, _) = create(&src, &paths, &m, &fx.path("out"), &fx.key).unwrap();
        assert_eq!(first, fs::read(p2).unwrap());
    }

    #[test]
    fn unsigned_or_tampered_refused() {
        let fx = Fixture::new();
        let p = build_pkg(&fx, &meta("hello", "1.0", &[]), "README");

        // Flip one byte.
        let mut bytes = fs::read(&p).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xff;
        let tampered = fx.path("out/t/hello-1.0-1.x86_64.fernpkg");
        fs::create_dir_all(tampered.parent().unwrap()).unwrap();
        fs::write(&tampered, &bytes).unwrap();
        fs::copy(sig_path(&p), sig_path(&tampered)).unwrap();
        let e = VerifiedPackage::open(&tampered, &fx.ring, None, &fx.work()).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");

        // No signature.
        fs::remove_file(sig_path(&tampered)).unwrap();
        fs::write(&tampered, fs::read(&p).unwrap()).unwrap();
        let e = VerifiedPackage::open(&tampered, &fx.ring, None, &fx.work()).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");

        // Signed by an untrusted key.
        let other = SecretKey::generate().unwrap();
        other.sign_file(Purpose::Package, &tampered).unwrap();
        let e = VerifiedPackage::open(&tampered, &fx.ring, None, &fx.work()).unwrap_err();
        assert!(e.to_string().contains("not trusted"), "{e}");

        // Signed as an index instead of a package.
        fx.key.sign_file(Purpose::Index, &tampered).unwrap();
        assert!(VerifiedPackage::open(&tampered, &fx.ring, None, &fx.work()).is_err());
    }

    #[test]
    fn index_mismatch_refused() {
        let fx = Fixture::new();
        let p = build_pkg(&fx, &meta("hello", "1.0", &[]), "README");
        let (d, n) = hash::sha256_file(&p).unwrap();
        let good = Expected {
            sha256: hex::encode(d),
            size: n,
            name: "hello".into(),
            version: PkgVersion::new(Version::parse("1.0").unwrap(), 1),
        };
        VerifiedPackage::open(&p, &fx.ring, Some(&good), &fx.work()).unwrap();

        let mut wrong = good.clone();
        wrong.sha256 = "f".repeat(64);
        assert!(matches!(
            VerifiedPackage::open(&p, &fx.ring, Some(&wrong), &fx.work()),
            Err(Error::Verify(_))
        ));
        let mut wrong = good.clone();
        wrong.version = PkgVersion::new(Version::parse("1.0").unwrap(), 2);
        assert!(matches!(
            VerifiedPackage::open(&p, &fx.ring, Some(&wrong), &fx.work()),
            Err(Error::Verify(_))
        ));
    }

    #[test]
    fn renamed_file_refused() {
        let fx = Fixture::new();
        let p = build_pkg(&fx, &meta("hello", "1.0", &[]), "README");
        let renamed = fx.path("out/hello-2.0-1.x86_64.fernpkg");
        fs::copy(&p, &renamed).unwrap();
        fs::copy(sig_path(&p), sig_path(&renamed)).unwrap();
        let e = VerifiedPackage::open(&renamed, &fx.ring, None, &fx.work()).unwrap_err();
        assert!(e.to_string().contains("file name"), "{e}");
    }

    /// Writes a signed archive with a valid manifest but a hand-made
    /// payload, as an attacker holding a signing key or exploiting a
    /// builder bug might.
    fn evil_pkg(
        fx: &Fixture,
        manifest: &Manifest,
        build: impl FnOnce(&mut tar::Builder<Vec<u8>>),
    ) -> PathBuf {
        let mut tb = tar::Builder::new(Vec::new());
        let text = manifest.to_toml().unwrap();
        let mut h = header(tar::EntryType::Regular, 0o644, text.len() as u64, 0);
        tb.append_data(&mut h, MANIFEST_NAME, text.as_bytes())
            .unwrap();
        build(&mut tb);
        let raw = tb.into_inner().unwrap();
        let compressed = zstd::encode_all(&raw[..], 3).unwrap();
        let dir = fx.path("evil");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(manifest.file_name());
        fs::write(&p, compressed).unwrap();
        fx.key.sign_file(Purpose::Package, &p).unwrap();
        p
    }

    fn dir_entry(path: &str) -> FileEntry {
        FileEntry {
            path: path.into(),
            kind: FileKind::Dir,
            mode: Some(0o755),
            size: None,
            sha256: None,
            target: None,
        }
    }

    fn base_manifest(files: Vec<FileEntry>) -> Manifest {
        let mut m = crate::manifest::tests::sample();
        m.files = files;
        m.installed_size = m.files.iter().filter_map(|f| f.size).sum();
        m
    }

    fn try_extract(fx: &Fixture, p: &Path) -> Result<()> {
        let vp = VerifiedPackage::open(p, &fx.ring, None, &fx.work())?;
        let dest = fx.path("dest-evil");
        let _ = fs::remove_dir_all(&dest);
        fs::create_dir(&dest).unwrap();
        vp.extract(&dest)
    }

    fn put(tb: &mut tar::Builder<Vec<u8>>, kind: tar::EntryType, path: &str, data: &[u8]) {
        let mut h = header(
            kind,
            if kind == tar::EntryType::Directory {
                0o755
            } else {
                0o644
            },
            data.len() as u64,
            0,
        );
        tb.append_data(&mut h, path, data).unwrap();
    }

    fn file_entry(path: &str, data: &[u8]) -> FileEntry {
        FileEntry {
            path: path.into(),
            kind: FileKind::File,
            mode: Some(0o644),
            size: Some(data.len() as u64),
            sha256: Some(hash::sha256_hex(data)),
            target: None,
        }
    }

    #[test]
    fn extra_payload_entry_refused() {
        let fx = Fixture::new();
        let m = base_manifest(vec![dir_entry("usr")]);
        let p = evil_pkg(&fx, &m, |tb| {
            put(tb, tar::EntryType::Directory, "usr", b"");
            put(tb, tar::EntryType::Regular, "usr/evil", b"x");
        });
        let e = try_extract(&fx, &p).unwrap_err();
        assert!(e.to_string().contains("not in the manifest"), "{e}");
    }

    #[test]
    fn traversal_refused() {
        let fx = Fixture::new();
        let m = base_manifest(vec![dir_entry("usr")]);
        let p = evil_pkg(&fx, &m, |tb| {
            put(tb, tar::EntryType::Directory, "usr", b"");
            // tar refuses to *write* `..` via append_data, so build the
            // header by hand.
            let mut h = header(tar::EntryType::Regular, 0o644, 1, 0);
            h.as_old_mut().name[..13].copy_from_slice(b"usr/../etc/x\0");
            h.set_cksum();
            tb.append(&h, &b"x"[..]).unwrap();
        });
        assert!(try_extract(&fx, &p).is_err());
        assert!(!fx.path("dest-evil/etc").exists());
    }

    #[test]
    fn content_mismatch_refused() {
        let fx = Fixture::new();
        let m = base_manifest(vec![dir_entry("usr"), file_entry("usr/a", b"good")]);
        let p = evil_pkg(&fx, &m, |tb| {
            put(tb, tar::EntryType::Directory, "usr", b"");
            put(tb, tar::EntryType::Regular, "usr/a", b"evil");
        });
        let e = try_extract(&fx, &p).unwrap_err();
        assert!(e.to_string().contains("hash"), "{e}");
    }

    #[test]
    fn missing_entry_refused() {
        let fx = Fixture::new();
        let m = base_manifest(vec![dir_entry("usr"), file_entry("usr/a", b"good")]);
        let p = evil_pkg(&fx, &m, |tb| {
            put(tb, tar::EntryType::Directory, "usr", b"");
        });
        let e = try_extract(&fx, &p).unwrap_err();
        assert!(e.to_string().contains("not the archive"), "{e}");
    }

    #[test]
    fn hardlink_and_kind_mismatch_refused() {
        let fx = Fixture::new();
        let m = base_manifest(vec![dir_entry("usr"), file_entry("usr/a", b"")]);
        let p = evil_pkg(&fx, &m, |tb| {
            put(tb, tar::EntryType::Directory, "usr", b"");
            let mut h = header(tar::EntryType::Link, 0o644, 0, 0);
            tb.append_link(&mut h, "usr/a", "/etc/shadow").unwrap();
        });
        assert!(try_extract(&fx, &p).is_err());
    }

    #[test]
    fn write_through_symlink_refused() {
        // usr/lib is a symlink in the manifest; a later entry tries to
        // write usr/lib/x through it.
        let fx = Fixture::new();
        let m = base_manifest(vec![
            dir_entry("usr"),
            FileEntry {
                path: "usr/lib".into(),
                kind: FileKind::Symlink,
                mode: None,
                size: None,
                sha256: None,
                target: Some("/tmp".into()),
            },
        ]);
        let p = evil_pkg(&fx, &m, |tb| {
            put(tb, tar::EntryType::Directory, "usr", b"");
            let mut h = header(tar::EntryType::Symlink, 0o777, 0, 0);
            tb.append_link(&mut h, "usr/lib", "/tmp").unwrap();
            put(tb, tar::EntryType::Regular, "usr/lib/x", b"x");
        });
        assert!(try_extract(&fx, &p).is_err());
    }

    #[test]
    fn duplicate_entry_refused() {
        let fx = Fixture::new();
        let m = base_manifest(vec![dir_entry("usr"), file_entry("usr/a", b"x")]);
        let p = evil_pkg(&fx, &m, |tb| {
            put(tb, tar::EntryType::Directory, "usr", b"");
            put(tb, tar::EntryType::Regular, "usr/a", b"x");
            put(tb, tar::EntryType::Regular, "usr/a", b"x");
        });
        assert!(try_extract(&fx, &p).is_err());
    }

    #[test]
    fn garbage_refused_after_signature() {
        let fx = Fixture::new();
        let p = fx.path("junk-1.0-1.x86_64.fernpkg");
        fs::write(&p, b"not zstd at all").unwrap();
        fx.key.sign_file(Purpose::Package, &p).unwrap();
        let e = VerifiedPackage::open(&p, &fx.ring, None, &fx.work()).unwrap_err();
        assert!(matches!(e, Error::Verify(_)), "{e}");
    }

    #[test]
    fn extract_requires_empty_dest() {
        let fx = Fixture::new();
        let p = build_pkg(&fx, &meta("hello", "1.0", &[]), "README");
        let vp = VerifiedPackage::open(&p, &fx.ring, None, &fx.work()).unwrap();
        let dest = fx.path("busy");
        fs::create_dir_all(dest.join("x")).unwrap();
        assert!(vp.extract(&dest).is_err());
    }

    #[test]
    fn describe_rejects_fifo() {
        let fx = Fixture::new();
        let src = fx.path("fifo");
        fs::create_dir_all(src.join("usr")).unwrap();
        let c = std::ffi::CString::new(src.join("usr/p").to_str().unwrap()).unwrap();
        // SAFETY: c is a valid NUL-terminated path.
        #[allow(unsafe_code)]
        let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o644) };
        assert_eq!(rc, 0);
        assert!(describe_files(&src, &["usr".into(), "usr/p".into()]).is_err());
    }
}
