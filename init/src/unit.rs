//! Service units (docs/formats/unit.md).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Error, Result};

/// Where units are read from, in order; a later file with the same name
/// wins.
pub const UNIT_DIRS: &[&str] = &["usr/lib/sol/units", "etc/sol/units"];
pub const ENABLED_DIR: &str = "etc/sol/enabled";
pub const DISABLED_DIR: &str = "etc/sol/disabled";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    #[default]
    Simple,
    Oneshot,
    Notify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Restart {
    No,
    #[default]
    OnFailure,
    Always,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub exec: Vec<String>,
    #[serde(default, rename = "type")]
    pub kind: ServiceType,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub restart: Restart,
    #[serde(default = "default_restart_delay")]
    pub restart_delay_ms: u64,
    #[serde(default = "default_stop_timeout")]
    pub stop_timeout_ms: u64,
}

fn default_restart_delay() -> u64 {
    500
}
fn default_stop_timeout() -> u64 {
    5000
}
fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sandbox {
    #[serde(default = "default_true")]
    pub no_new_privs: bool,
    #[serde(default)]
    pub read_only_paths: Vec<PathBuf>,
}

impl Default for Sandbox {
    fn default() -> Self {
        Sandbox {
            no_new_privs: true,
            read_only_paths: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default)]
    pub memory_max: Option<String>,
    #[serde(default)]
    pub tasks_max: Option<u32>,
    #[serde(default)]
    pub cpu_weight: Option<u32>,
}

impl Limits {
    /// `memory_max` in bytes.
    ///
    /// # Errors
    /// A malformed size.
    pub fn memory_max_bytes(&self) -> Result<Option<u64>> {
        self.memory_max.as_deref().map(parse_size).transpose()
    }
}

/// `"512M"` -> bytes. Suffixes K, M, G (powers of 1024).
///
/// # Errors
/// Empty, non-numeric, or overflowing sizes.
pub fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('K' | 'k') => (&s[..s.len() - 1], 1u64 << 10),
        Some('M' | 'm') => (&s[..s.len() - 1], 1 << 20),
        Some('G' | 'g') => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    num.parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(mult))
        .ok_or_else(|| Error::Unit(format!("bad size {s:?}")))
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    #[serde(skip)]
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
    pub service: Service,
    #[serde(default)]
    pub sandbox: Sandbox,
    #[serde(default)]
    pub limits: Limits,
    /// The file it came from.
    #[serde(skip)]
    pub path: PathBuf,
}

/// `[a-z0-9][a-z0-9@._-]*`, at most 64 bytes.
///
/// # Errors
/// [`Error::Unit`] for an invalid name.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'@' | b'.' | b'_' | b'-')
        });
    if ok {
        Ok(())
    } else {
        Err(Error::Unit(format!("invalid unit name {name:?}")))
    }
}

impl Unit {
    /// Parses unit text. `name` comes from the file name.
    ///
    /// # Errors
    /// TOML and validation errors.
    pub fn parse(name: &str, text: &str) -> Result<Self> {
        validate_name(name)?;
        let mut u: Unit = toml::from_str(text).map_err(|e| Error::Unit(format!("{name}: {e}")))?;
        u.name = name.to_string();
        u.validate()?;
        Ok(u)
    }

    fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::Unit(format!("{}: {m}", self.name)));
        if self.service.exec.is_empty() {
            return bad("service.exec is empty".into());
        }
        if !self.service.exec[0].starts_with('/') {
            return bad("service.exec[0] must be an absolute path".into());
        }
        for d in self.requires.iter().chain(&self.after) {
            validate_name(d)?;
            if d == &self.name {
                return bad("a unit cannot depend on itself".into());
            }
        }
        if self.service.kind == ServiceType::Oneshot && self.service.restart == Restart::Always {
            return bad("oneshot units cannot use restart = \"always\"".into());
        }
        for p in &self.sandbox.read_only_paths {
            if !p.is_absolute() {
                return bad(format!(
                    "read_only_paths entry {} is not absolute",
                    p.display()
                ));
            }
        }
        if let Some(w) = self.limits.cpu_weight
            && !(1..=10_000).contains(&w)
        {
            return bad("cpu_weight must be 1..10000".into());
        }
        self.limits.memory_max_bytes()?;
        for k in self.service.environment.keys() {
            if k.is_empty() || k.contains('=') || k.contains('\0') {
                return bad(format!("bad environment variable name {k:?}"));
            }
        }
        Ok(())
    }
}

/// Loads every unit, later directories overriding earlier ones. A unit
/// that cannot be read or parsed, or that requires a unit which does not
/// exist (or was itself skipped), is left out and reported in the second
/// list: PID 1 boots everything else rather than halting on one bad file.
pub fn load_all(root: &Path) -> (BTreeMap<String, Unit>, Vec<String>) {
    let mut out = BTreeMap::new();
    let mut problems = Vec::new();
    for dir in UNIT_DIRS {
        let dir = root.join(dir);
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut paths: Vec<PathBuf> = rd
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        paths.sort();
        for p in paths {
            let loaded = (|| {
                let name = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| Error::Unit("bad file name".into()))?
                    .to_string();
                let text = fs::read_to_string(&p).map_err(|e| Error::io(&p, &e))?;
                let mut u = Unit::parse(&name, &text)?;
                u.path.clone_from(&p);
                Ok::<_, Error>(u)
            })();
            match loaded {
                Ok(u) => {
                    out.insert(u.name.clone(), u);
                }
                Err(e) => problems.push(format!("{}: {e}", p.display())),
            }
        }
    }
    // Drop units whose requirements are missing, until none are.
    loop {
        let broken: Vec<(String, String)> = out
            .values()
            .filter_map(|u| {
                u.requires
                    .iter()
                    .find(|d| !out.contains_key(*d))
                    .map(|d| (u.name.clone(), d.clone()))
            })
            .collect();
        if broken.is_empty() {
            break;
        }
        for (name, dep) in broken {
            out.remove(&name);
            problems.push(format!("{name} requires {dep}, which does not exist"));
        }
    }
    (out, problems)
}

/// Units that start at boot: `enabled = true` or listed in
/// `/etc/sol/enabled`, minus those in `/etc/sol/disabled`.
pub fn enabled_set(root: &Path, units: &BTreeMap<String, Unit>) -> BTreeSet<String> {
    let names_in = |dir: &str| -> BTreeSet<String> {
        fs::read_dir(root.join(dir))
            .map(|rd| {
                rd.filter_map(std::result::Result::ok)
                    .filter_map(|e| e.file_name().to_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let on = names_in(ENABLED_DIR);
    let off = names_in(DISABLED_DIR);
    units
        .values()
        .filter(|u| (u.enabled || on.contains(&u.name)) && !off.contains(&u.name))
        .map(|u| u.name.clone())
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {

    /// Every unit shipped in this repository (recipes' `units/` and the
    /// base image) parses, and everything it requires or orders after is
    /// shipped too.
    #[test]
    fn shipped_units_are_valid() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut dirs = vec![repo.join("image/base/usr/lib/sol/units")];
        for r in fs::read_dir(repo.join("recipes")).unwrap() {
            dirs.push(r.unwrap().path().join("units"));
        }
        let mut units = BTreeMap::new();
        for dir in dirs {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for f in rd {
                let path = f.unwrap().path();
                let name = path.file_stem().unwrap().to_str().unwrap().to_string();
                let text = fs::read_to_string(&path).unwrap();
                let unit =
                    Unit::parse(&name, &text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                units.insert(name, unit);
            }
        }
        assert!(units.contains_key("udevd") && units.contains_key("sol-comp"));
        for u in units.values() {
            for d in u.requires.iter().chain(&u.after) {
                assert!(units.contains_key(d), "{} names unknown unit {d}", u.name);
            }
        }
    }

    use super::*;

    pub fn unit(name: &str, requires: &[&str]) -> Unit {
        let reqs: Vec<String> = requires.iter().map(|r| format!("\"{r}\"")).collect();
        Unit::parse(
            name,
            &format!(
                "description = \"{name}\"\nrequires = [{}]\n[service]\nexec = [\"/bin/true\"]\n",
                reqs.join(",")
            ),
        )
        .unwrap()
    }

    #[test]
    fn parses_full_unit() {
        let u = Unit::parse(
            "getty-tty1",
            r#"
description = "Login prompt"
enabled = true
requires = ["early"]
after = ["udev"]
[service]
exec = ["/usr/sbin/agetty", "--noclear", "tty1"]
type = "simple"
restart = "always"
environment = { TERM = "linux" }
[sandbox]
no_new_privs = false
read_only_paths = ["/usr"]
[limits]
memory_max = "64M"
tasks_max = 16
cpu_weight = 50
"#,
        )
        .unwrap();
        assert_eq!(u.service.restart, Restart::Always);
        assert_eq!(u.limits.memory_max_bytes().unwrap(), Some(64 << 20));
        assert!(!u.sandbox.no_new_privs);
        assert_eq!(u.service.stop_timeout_ms, 5000);
    }

    #[test]
    fn defaults() {
        let u = unit("a", &[]);
        assert_eq!(u.service.kind, ServiceType::Simple);
        assert_eq!(u.service.restart, Restart::OnFailure);
        assert!(u.sandbox.no_new_privs);
        assert!(!u.enabled);
    }

    #[test]
    fn rejects_bad_units() {
        let base = "description = \"x\"\n[service]\nexec = [\"/bin/true\"]\n";
        for (name, text, want) in [
            ("Bad", base.to_string(), "invalid unit name"),
            ("a", base.replace("/bin/true", "true"), "absolute"),
            (
                "a",
                base.replace("exec = [\"/bin/true\"]", "exec = []"),
                "empty",
            ),
            ("a", format!("requires = [\"a\"]\n{base}"), "itself"),
            (
                "a",
                format!("{base}type = \"oneshot\"\nrestart = \"always\"\n"),
                "oneshot",
            ),
            (
                "a",
                format!("{base}[limits]\nmemory_max = \"lots\"\n"),
                "bad size",
            ),
            (
                "a",
                format!("{base}[limits]\ncpu_weight = 0\n"),
                "cpu_weight",
            ),
            ("a", format!("{base}bogus = 1\n"), "unknown field"),
            (
                "a",
                format!("{base}environment = {{ \"A=B\" = \"x\" }}\n"),
                "environment",
            ),
        ] {
            let e = Unit::parse(name, &text).unwrap_err().to_string();
            assert!(e.contains(want), "{want}: {e}");
        }
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert_eq!(parse_size("2K").unwrap(), 2048);
        assert_eq!(parse_size("1G").unwrap(), 1 << 30);
        assert!(parse_size("").is_err());
        assert!(parse_size("99999999999999999999G").is_err());
    }

    #[test]
    fn loading_overrides_and_enabled_set() {
        let d = tempfile::tempdir().unwrap();
        let w = |p: &str, t: &str| {
            let p = d.path().join(p);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, t).unwrap();
        };
        let u = |desc: &str, extra: &str| {
            format!("description = \"{desc}\"\n{extra}\n[service]\nexec = [\"/bin/true\"]\n")
        };
        w("usr/lib/sol/units/a.toml", &u("shipped", "enabled = true"));
        w("etc/sol/units/a.toml", &u("override", "enabled = true"));
        w("usr/lib/sol/units/b.toml", &u("b", ""));
        w("usr/lib/sol/units/c.toml", &u("c", "enabled = true"));
        w("usr/lib/sol/units/README", "ignored");
        w("etc/sol/enabled/b", "");
        w("etc/sol/disabled/c", "");
        let (all, problems) = load_all(d.path());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(all["a"].description, "override");
        let en = enabled_set(d.path(), &all);
        assert_eq!(en.into_iter().collect::<Vec<_>>(), ["a", "b"]);

        // A unit with a missing requirement is skipped, and so is one
        // that requires it; a malformed file is skipped. The rest load.
        w(
            "usr/lib/sol/units/x.toml",
            &u("x", "requires = [\"missing\"]"),
        );
        w("usr/lib/sol/units/y.toml", &u("y", "requires = [\"x\"]"));
        w("usr/lib/sol/units/Bad-Name.toml", &u("bad", ""));
        let (all, problems) = load_all(d.path());
        assert!(!all.contains_key("x") && !all.contains_key("y"));
        assert!(all.contains_key("a") && all.contains_key("b"));
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("missing")));
        assert!(problems.iter().any(|p| p.contains("Bad-Name")));
    }
}
