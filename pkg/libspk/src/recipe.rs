//! Recipes: `recipes/<name>/recipe.toml` (docs/formats/recipe.md).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, IoContext, Result};
use crate::hash;
use crate::manifest::is_sha256_hex;
use crate::version::{DepSpec, PkgVersion, Version, validate_name};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub name: String,
    pub version: Version,
    pub release: u32,
    pub summary: String,
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(rename = "source")]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub deps: Deps,
    pub build: Build,
    #[serde(default, rename = "split")]
    pub splits: Vec<Split>,
    #[serde(default)]
    pub package: PackageRels,
    #[serde(default)]
    pub triggers: Triggers,

    /// Directory the recipe was loaded from (not part of the file).
    #[serde(skip)]
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Upstream source: `https://` URL + sha256.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    /// Or our own code: a directory relative to the repository root,
    /// copied in as `/build/src/<basename>` (no hash: it is part of the
    /// repo, and its contents are hashed into the build stamp).
    #[serde(default)]
    pub local: Option<String>,
    #[serde(default)]
    pub mirrors: Vec<String>,
    #[serde(default = "default_true")]
    pub extract: bool,
    #[serde(default)]
    pub dir: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Source {
    /// The file name the source is stored under in the work dir (for a
    /// local source, the directory name).
    pub fn file_name(&self) -> &str {
        match &self.local {
            Some(l) => l
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("source"),
            None => self.url.rsplit('/').next().unwrap_or("source"),
        }
    }

    pub fn is_local(&self) -> bool {
        self.local.is_some()
    }
}

/// A local source path: relative, normalised, no `..`.
fn valid_local(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && p.trim_end_matches('/')
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains('\\'))
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deps {
    #[serde(default)]
    pub build: Vec<DepSpec>,
    #[serde(default)]
    pub runtime: Vec<DepSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildSystem {
    Autotools,
    Cmake,
    Make,
    Custom,
}

/// Where a build's compilers and tools come from (docs/formats/recipe.md,
/// "Build environment").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolchainMode {
    /// The host's (Nix devShell's) tools, with the sysroot's headers and
    /// libraries added to the search paths. Phase 1 scaffolding.
    Host,
    /// The host's tools for the *build* machine plus a cross toolchain
    /// installed in the sysroot (`/usr/cross/bin`); no sysroot search
    /// flags. Bootstrap stages 0 and 1.
    Cross,
    /// Only our own packages: the sandbox has no `/nix`, and every tool
    /// comes from the sysroot. Everything that ships is built this way.
    Native,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub system: BuildSystem,
    /// Overrides the build stage's default toolchain mode.
    #[serde(default)]
    pub toolchain: Option<ToolchainMode>,
    #[serde(default)]
    pub configure_args: Vec<String>,
    #[serde(default)]
    pub make_args: Vec<String>,
    #[serde(default)]
    pub install_args: Vec<String>,
    #[serde(default)]
    pub prepare: Option<String>,
    #[serde(default)]
    pub configure: Option<String>,
    #[serde(default)]
    pub compile: Option<String>,
    #[serde(default)]
    pub install: Option<String>,
    #[serde(default)]
    pub post_install: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Split {
    pub name: String,
    pub summary: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub depends: Vec<DepSpec>,
    #[serde(default)]
    pub provides: Vec<DepSpec>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRels {
    #[serde(default)]
    pub provides: Vec<DepSpec>,
    #[serde(default)]
    pub conflicts: Vec<DepSpec>,
    #[serde(default)]
    pub replaces: Vec<DepSpec>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Triggers {
    #[serde(default)]
    pub post_install: Vec<String>,
}

/// Quotes a string for POSIX sh.
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:+,@%".contains(&b))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| sh_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Recipe {
    /// Loads `<dir>/recipe.toml`.
    ///
    /// # Errors
    /// I/O errors, TOML errors, and validation failures.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("recipe.toml");
        let text = fs::read_to_string(&path).at(&path)?;
        let mut r: Recipe = toml::from_str(&text)
            .map_err(|e| Error::invalid(format!("{}: {e}", path.display())))?;
        r.dir = dir.to_path_buf();
        r.validate()
            .map_err(|e| Error::invalid(format!("{}: {e}", path.display())))?;
        Ok(r)
    }

    /// # Errors
    /// Returns [`Error::Invalid`] for the first problem found.
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        if let Some(dirname) = self.dir.file_name().and_then(|n| n.to_str())
            && dirname != self.name
        {
            return Err(Error::invalid(format!(
                "recipe name {} does not match its directory {dirname}",
                self.name
            )));
        }
        if self.release == 0 {
            return Err(Error::invalid("release must be at least 1"));
        }
        if self.summary.trim().is_empty() || self.license.trim().is_empty() {
            return Err(Error::invalid("summary and license are required"));
        }
        if self.sources.is_empty() {
            return Err(Error::invalid("at least one [[source]] is required"));
        }
        for s in &self.sources {
            if let Some(l) = &s.local {
                if !s.url.is_empty() || !s.sha256.is_empty() || !s.mirrors.is_empty() {
                    return Err(Error::invalid(format!(
                        "source {l}: a local source has no url, sha256, or mirrors"
                    )));
                }
                if !valid_local(l) {
                    return Err(Error::invalid(format!(
                        "local source {l:?} must be a relative path inside the repository"
                    )));
                }
                continue;
            }
            for url in std::iter::once(&s.url).chain(&s.mirrors) {
                if !url.starts_with("https://") {
                    return Err(Error::invalid(format!(
                        "source URL must be https://: {url}"
                    )));
                }
            }
            if !is_sha256_hex(&s.sha256) {
                return Err(Error::invalid(format!(
                    "source {} needs a lowercase sha256",
                    s.url
                )));
            }
            let f = s.file_name();
            if f.is_empty() || f == "." || f == ".." || f.contains(['\\', '\0']) {
                return Err(Error::invalid(format!("bad source file name in {}", s.url)));
            }
        }
        if self.build.system == BuildSystem::Custom
            && (self.build.compile.is_none() || self.build.install.is_none())
        {
            return Err(Error::invalid(
                "build.system = \"custom\" needs at least compile and install",
            ));
        }
        let mut names = vec![self.name.as_str()];
        for s in &self.splits {
            validate_name(&s.name)?;
            if names.contains(&s.name.as_str()) {
                return Err(Error::invalid(format!("duplicate package name {}", s.name)));
            }
            names.push(&s.name);
            if s.files.is_empty() {
                return Err(Error::invalid(format!("split {} claims no files", s.name)));
            }
            for g in &s.files {
                glob::Pattern::new(g)
                    .map_err(|e| Error::invalid(format!("split {} glob {g:?}: {e}", s.name)))?;
            }
        }
        Ok(())
    }

    pub fn pkg_version(&self) -> PkgVersion {
        PkgVersion::new(self.version.clone(), self.release)
    }

    /// Names of every package this recipe produces (main first).
    pub fn package_names(&self) -> Vec<String> {
        std::iter::once(self.name.clone())
            .chain(self.splits.iter().map(|s| s.name.clone()))
            .collect()
    }

    /// sha256 over every file in the recipe dir (sorted relative paths and
    /// contents), recorded in each manifest so a package can be traced to
    /// the exact recipe that built it.
    ///
    /// # Errors
    /// I/O errors while reading the recipe dir.
    pub fn content_hash(&self) -> Result<String> {
        let mut entries = Vec::new();
        for e in walkdir::WalkDir::new(&self.dir).sort_by_file_name() {
            let e = e.map_err(|e| Error::invalid(format!("{}: {e}", self.dir.display())))?;
            if e.file_type().is_file() {
                let rel = e
                    .path()
                    .strip_prefix(&self.dir)
                    .unwrap_or(e.path())
                    .to_string_lossy()
                    .into_owned();
                let data = fs::read(e.path()).at(e.path())?;
                entries.push((rel, hash::sha256_hex(&data)));
            }
        }
        let mut text = String::new();
        for (rel, h) in entries {
            let _ = writeln!(text, "{h}  {rel}");
        }
        Ok(hash::sha256_hex(text.as_bytes()))
    }

    /// The shell script run inside the build sandbox, from the source dir.
    pub fn build_script(&self) -> String {
        let b = &self.build;
        let (configure, compile, install) = match b.system {
            BuildSystem::Autotools => (
                format!(
                    "./configure --prefix=/usr --sysconfdir=/etc --localstatedir=/var \
                     --libdir=/usr/lib {}",
                    join_args(&b.configure_args)
                ),
                format!("make {}", join_args(&b.make_args)),
                format!(
                    "make DESTDIR=\"$DESTDIR\" install {}",
                    join_args(&b.install_args)
                ),
            ),
            BuildSystem::Cmake => (
                format!(
                    "cmake -S . -B _build -G Ninja -DCMAKE_INSTALL_PREFIX=/usr \
                     -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_BUILD_TYPE=Release {}",
                    join_args(&b.configure_args)
                ),
                format!("ninja -C _build {}", join_args(&b.make_args)),
                format!(
                    "DESTDIR=\"$DESTDIR\" ninja -C _build install {}",
                    join_args(&b.install_args)
                ),
            ),
            BuildSystem::Make => (
                String::new(),
                format!("make PREFIX=/usr {}", join_args(&b.make_args)),
                format!(
                    "make PREFIX=/usr DESTDIR=\"$DESTDIR\" install {}",
                    join_args(&b.install_args)
                ),
            ),
            BuildSystem::Custom => (String::new(), String::new(), String::new()),
        };
        let phase = |name: &str, over: &Option<String>, default: String| -> String {
            let body = over.clone().unwrap_or(default);
            if body.trim().is_empty() {
                format!("# {name}: (none)\n")
            } else {
                // Each phase starts in the source dir: a `cd` in one phase
                // must not leak into the next.
                format!(
                    "echo '==> {name}'\n(\nset -euo pipefail\n{}\n)\n",
                    body.trim_end()
                )
            }
        };
        let mut s = String::from("set -euo pipefail\n");
        s += &phase("prepare", &b.prepare, String::new());
        s += &phase("configure", &b.configure, configure);
        s += &phase("compile", &b.compile, compile);
        s += &phase("install", &b.install, install);
        s += &phase("post_install", &b.post_install, String::new());
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
name = "xz"
version = "5.8.1"
release = 1
summary = "XZ tools"
license = "0BSD"

[[source]]
url = "https://example.org/xz-5.8.1.tar.xz"
sha256 = "0b54f79df85912504de0b14aec7971e3f964491af1812d83447005807513cd9e"

[deps]
build = ["zlib>=1.3"]

[build]
system = "autotools"
configure_args = ["--disable-static", "CFLAGS=-O2 -g"]

[[split]]
name = "liblzma"
summary = "LZMA library"
files = ["usr/lib/liblzma.so*"]
"#;

    fn parse(text: &str) -> Result<Recipe> {
        let mut r: Recipe = toml::from_str(text).map_err(|e| Error::invalid(e.to_string()))?;
        r.dir = PathBuf::from("recipes/xz");
        r.validate()?;
        Ok(r)
    }

    #[test]
    fn parses_and_scripts() {
        let r = parse(GOOD).unwrap();
        assert_eq!(r.package_names(), ["xz", "liblzma"]);
        assert_eq!(r.sources[0].file_name(), "xz-5.8.1.tar.xz");
        let s = r.build_script();
        assert!(s.starts_with("set -euo pipefail\n"));
        assert!(s.contains("--libdir=/usr/lib --disable-static 'CFLAGS=-O2 -g'"));
        assert!(s.contains("make DESTDIR=\"$DESTDIR\" install"));
    }

    #[test]
    fn rejects() {
        let cases = [
            (GOOD.replace("https://", "http://"), "https"),
            (GOOD.replace("0b54f7", "0B54F7"), "sha256"),
            (
                GOOD.replace("name = \"xz\"", "name = \"other\""),
                "directory",
            ),
            (GOOD.replace("release = 1", "release = 0"), "release"),
            (
                GOOD.replace("name = \"liblzma\"", "name = \"xz\""),
                "duplicate",
            ),
            (GOOD.replace("[deps]", "[deps]\nfoo = 1"), "unknown field"),
            (GOOD.replace("autotools", "custom"), "custom"),
        ];
        for (text, want) in cases {
            let err = parse(&text).unwrap_err().to_string();
            assert!(err.contains(want), "{want}: {err}");
        }
    }

    #[test]
    fn local_sources() {
        let ok = GOOD.replace(
            "[[source]]\nurl",
            "[[source]]\nlocal = \"boot/grub-theme\"\n\n[[source]]\nurl",
        );
        let r = parse(&ok).unwrap();
        assert!(r.sources[0].is_local());
        assert_eq!(r.sources[0].file_name(), "grub-theme");
        for bad in ["../etc", "/abs", "a/../b", ""] {
            let t = GOOD.replace(
                "[[source]]\nurl",
                &format!("[[source]]\nlocal = \"{bad}\"\n\n[[source]]\nurl"),
            );
            assert!(parse(&t).is_err(), "{bad}");
        }
        let both = GOOD.replace("[[source]]\nurl", "[[source]]\nlocal = \"x\"\nurl");
        assert!(parse(&both).is_err());
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("--x=y"), "--x=y");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(sh_quote("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(sh_quote(""), "''");
    }
}
