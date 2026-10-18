//! The package manifest (`.MANIFEST` inside every `.solpkg`) and the path
//! rules every payload path must satisfy.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::version::{DepSpec, PkgVersion, Version, validate_name};

pub const FORMAT: u32 = 1;
pub const ARCH: &str = "x86_64";

/// Checks a payload path: relative, normalised, UTF-8, under `usr/`.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the path and the rule it breaks.
pub fn validate_payload_path(path: &str) -> Result<()> {
    let bad = |why: &str| Err(Error::invalid(format!("bad package path {path:?}: {why}")));
    if path.is_empty() || path.len() > 4000 {
        return bad("empty or too long");
    }
    if path.starts_with('/') {
        return bad("absolute");
    }
    if path.chars().any(char::is_control) {
        return bad("control characters");
    }
    for comp in path.split('/') {
        match comp {
            "" => return bad("empty component or trailing slash"),
            "." | ".." => return bad("`.` or `..` component"),
            _ => {}
        }
    }
    if path != "usr" && !path.starts_with("usr/") {
        return bad("outside usr/");
    }
    Ok(())
}

/// Checks that a symlink at `link` pointing at `target` stays inside the
/// root when resolved lexically. Absolute targets are resolved against the
/// root, so only `..` components can escape.
///
/// # Errors
/// Returns [`Error::Invalid`] if the target escapes or is malformed.
pub fn validate_symlink_target(link: &str, target: &str) -> Result<()> {
    if target.is_empty() || target.contains('\0') || target.chars().any(char::is_control) {
        return Err(Error::invalid(format!(
            "bad symlink target {target:?} for {link}"
        )));
    }
    let mut depth: i64 = if target.starts_with('/') {
        0
    } else {
        // Depth of the link's parent directory.
        i64::try_from(link.split('/').count()).unwrap_or(i64::MAX) - 1
    };
    for comp in target.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return Err(Error::invalid(format!(
                        "symlink {link} -> {target} escapes the root"
                    )));
                }
            }
            _ => depth += 1,
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    File,
    Dir,
    Symlink,
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FileKind::File => "file",
            FileKind::Dir => "dir",
            FileKind::Symlink => "symlink",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub path: String,
    pub kind: FileKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl FileEntry {
    /// # Errors
    /// Returns [`Error::Invalid`] if fields are inconsistent with `kind`.
    pub fn validate(&self) -> Result<()> {
        validate_payload_path(&self.path)?;
        let bad = |why: &str| {
            Err(Error::invalid(format!(
                "manifest entry {}: {why}",
                self.path
            )))
        };
        match self.kind {
            FileKind::File => {
                let (Some(mode), Some(_), Some(hash)) = (self.mode, self.size, &self.sha256) else {
                    return bad("file needs mode, size, sha256");
                };
                if mode > 0o7777 {
                    return bad("mode out of range");
                }
                if !is_sha256_hex(hash) {
                    return bad("sha256 must be 64 lowercase hex chars");
                }
                if self.target.is_some() {
                    return bad("file must not have a target");
                }
            }
            FileKind::Dir => {
                let Some(mode) = self.mode else {
                    return bad("dir needs mode");
                };
                if mode > 0o7777 || mode & 0o6000 != 0 {
                    return bad("dir mode out of range or setuid/setgid");
                }
                if self.size.is_some() || self.sha256.is_some() || self.target.is_some() {
                    return bad("dir must only have a mode");
                }
            }
            FileKind::Symlink => {
                let Some(target) = &self.target else {
                    return bad("symlink needs target");
                };
                if self.mode.is_some() || self.size.is_some() || self.sha256.is_some() {
                    return bad("symlink must only have a target");
                }
                validate_symlink_target(&self.path, target)?;
            }
        }
        Ok(())
    }
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub name: String,
    pub version: Version,
    pub release: u32,
    pub arch: String,
    pub summary: String,
    pub license: String,
    #[serde(default)]
    pub depends: Vec<DepSpec>,
    #[serde(default)]
    pub provides: Vec<DepSpec>,
    #[serde(default)]
    pub conflicts: Vec<DepSpec>,
    #[serde(default)]
    pub replaces: Vec<DepSpec>,
    pub build_date: u64,
    pub recipe_sha256: String,
    pub installed_size: u64,
    #[serde(default)]
    pub files: Vec<FileEntry>,
}

impl Manifest {
    pub fn pkg_version(&self) -> PkgVersion {
        PkgVersion::new(self.version.clone(), self.release)
    }

    /// `<name>-<version>-<release>.<arch>.solpkg`
    pub fn file_name(&self) -> String {
        package_file_name(&self.name, &self.pkg_version(), &self.arch)
    }

    /// # Errors
    /// Returns [`Error::Invalid`] for the first problem found.
    pub fn parse(text: &str) -> Result<Self> {
        let m: Manifest =
            toml::from_str(text).map_err(|e| Error::invalid(format!("manifest: {e}")))?;
        m.validate()?;
        Ok(m)
    }

    /// # Errors
    /// Returns [`Error::Invalid`] if the manifest cannot be serialised.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string(self).map_err(|e| Error::invalid(format!("manifest: {e}")))
    }

    /// # Errors
    /// Returns [`Error::Invalid`] for the first problem found.
    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT {
            return Err(Error::invalid(format!(
                "manifest format {} is not supported (expected {FORMAT})",
                self.format
            )));
        }
        validate_name(&self.name)?;
        if self.arch != ARCH {
            return Err(Error::invalid(format!("unsupported arch {}", self.arch)));
        }
        if self.release == 0 {
            return Err(Error::invalid("release must be at least 1"));
        }
        if !is_sha256_hex(&self.recipe_sha256) {
            return Err(Error::invalid("recipe_sha256 must be a sha256 hex digest"));
        }
        let mut seen = HashSet::new();
        for f in &self.files {
            f.validate()?;
            if !seen.insert(f.path.as_str()) {
                return Err(Error::invalid(format!("duplicate path {}", f.path)));
            }
        }
        // Every entry's parent directory must be listed (or be the root),
        // so extraction never has to invent directories.
        for f in &self.files {
            if let Some((parent, _)) = f.path.rsplit_once('/') {
                let ok = self
                    .files
                    .iter()
                    .any(|d| d.path == parent && d.kind == FileKind::Dir);
                if !ok {
                    return Err(Error::invalid(format!(
                        "{} is listed but its directory {parent} is not",
                        f.path
                    )));
                }
            }
        }
        Ok(())
    }
}

pub fn package_file_name(name: &str, v: &PkgVersion, arch: &str) -> String {
    format!("{name}-{v}.{arch}.solpkg")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn sample() -> Manifest {
        let h = "a".repeat(64);
        Manifest {
            format: FORMAT,
            name: "demo".into(),
            version: Version::parse("1.0").unwrap(),
            release: 1,
            arch: ARCH.into(),
            summary: "demo".into(),
            license: "MIT".into(),
            depends: vec![DepSpec::parse("zlib>=1.3").unwrap()],
            provides: vec![],
            conflicts: vec![],
            replaces: vec![],
            build_date: 0,
            recipe_sha256: h.clone(),
            installed_size: 3,
            files: vec![
                FileEntry {
                    path: "usr".into(),
                    kind: FileKind::Dir,
                    mode: Some(0o755),
                    size: None,
                    sha256: None,
                    target: None,
                },
                FileEntry {
                    path: "usr/bin".into(),
                    kind: FileKind::Dir,
                    mode: Some(0o755),
                    size: None,
                    sha256: None,
                    target: None,
                },
                FileEntry {
                    path: "usr/bin/demo".into(),
                    kind: FileKind::File,
                    mode: Some(0o755),
                    size: Some(3),
                    sha256: Some(h),
                    target: None,
                },
                FileEntry {
                    path: "usr/bin/demo2".into(),
                    kind: FileKind::Symlink,
                    mode: None,
                    size: None,
                    sha256: None,
                    target: Some("demo".into()),
                },
            ],
        }
    }

    #[test]
    fn roundtrip() {
        let m = sample();
        let text = m.to_toml().unwrap();
        assert_eq!(Manifest::parse(&text).unwrap(), m);
        assert_eq!(m.file_name(), "demo-1.0-1.x86_64.solpkg");
    }

    #[test]
    fn payload_paths() {
        for good in [
            "usr",
            "usr/bin/x",
            "usr/share/factory/etc/x.conf",
            "usr/lib/a b",
        ] {
            validate_payload_path(good).unwrap();
        }
        for bad in [
            "",
            "/usr/bin/x",
            "etc/passwd",
            "usr/../etc",
            "usr/./x",
            "usr//x",
            "usr/",
            "usrx/y",
            "usr/x\n",
            "var/lib/x",
        ] {
            assert!(validate_payload_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn symlink_targets() {
        validate_symlink_target("usr/bin/a", "b").unwrap();
        validate_symlink_target("usr/bin/a", "../lib/x").unwrap();
        validate_symlink_target("usr/bin/a", "../../usr/lib/x").unwrap();
        validate_symlink_target("usr/bin/a", "/usr/lib/x").unwrap();
        assert!(validate_symlink_target("usr/bin/a", "../../../etc/shadow").is_err());
        assert!(validate_symlink_target("usr/a", "/../x").is_err());
        assert!(validate_symlink_target("usr/a", "").is_err());
    }

    #[test]
    fn rejects_bad_manifests() {
        let mut m = sample();
        m.files[2].sha256 = Some("XYZ".into());
        assert!(m.validate().is_err());

        let mut m = sample();
        m.files.push(m.files[2].clone());
        assert!(m.validate().unwrap_err().to_string().contains("duplicate"));

        let mut m = sample();
        m.files.remove(1); // usr/bin dir missing
        assert!(m.validate().unwrap_err().to_string().contains("directory"));

        let mut m = sample();
        m.files[3].target = Some("../../../../etc".into());
        assert!(m.validate().is_err());

        let mut m = sample();
        m.files[1].mode = Some(0o4755);
        assert!(m.validate().is_err());

        let mut m = sample();
        m.release = 0;
        assert!(m.validate().is_err());

        let text = sample()
            .to_toml()
            .unwrap()
            .replace("format = 1", "format = 1\nevil = 1");
        assert!(Manifest::parse(&text).is_err());
    }
}
