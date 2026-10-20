//! Settings storage: one TOML file per domain, `<dir>/<domain>.toml`, with
//! only the keys that were set (a missing key reads as its default).
//! Writes are atomic (temporary file, fsync, rename). Keys the schema does
//! not know are kept as they are, so an older daemon does not drop a newer
//! one's settings; values of the wrong type are ignored with a warning.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::Value;
use crate::schema::{self, Domain, Kind, Scope};

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    UnknownDomain(String),
    UnknownKey(String, String),
    Invalid(String),
    Io(String),
    /// Saved, but applying it failed; the previous value was put back.
    Apply(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnknownDomain(d) => write!(f, "no settings domain {d:?}"),
            Error::UnknownKey(d, k) => write!(f, "no key {k:?} in {d}"),
            Error::Invalid(m) | Error::Io(m) => f.write_str(m),
            Error::Apply(m) => write!(f, "could not apply the change: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Run after a domain's file changed; an error undoes the change.
pub type Apply<'a> = &'a mut dyn FnMut(&Path) -> std::result::Result<(), String>;

#[derive(Debug, Default, Clone, PartialEq)]
struct Data {
    set: BTreeMap<&'static str, Value>,
    /// Keys the schema does not know, written back unchanged.
    extra: toml::Table,
}

#[derive(Debug)]
pub struct Store {
    scope: Scope,
    dir: PathBuf,
    data: BTreeMap<&'static str, Data>,
}

impl Store {
    /// Loads every domain of `scope` from `dir`. Problems with a file are
    /// returned as warnings: the affected keys read as their defaults.
    pub fn open(scope: Scope, dir: impl Into<PathBuf>) -> (Store, Vec<String>) {
        let mut s = Store {
            scope,
            dir: dir.into(),
            data: BTreeMap::new(),
        };
        let mut warnings = Vec::new();
        for d in schema::domains(scope) {
            let (data, w) = load(&s.path(d.name), d);
            s.data.insert(d.name, data);
            warnings.extend(w);
        }
        (s, warnings)
    }

    pub fn scope(&self) -> Scope {
        self.scope
    }

    pub fn path(&self, domain: &str) -> PathBuf {
        self.dir.join(format!("{domain}.toml"))
    }

    pub fn domains(&self) -> impl Iterator<Item = &'static Domain> + use<> {
        schema::domains(self.scope)
    }

    fn domain(&self, name: &str) -> Result<&'static Domain> {
        schema::domain(name)
            .filter(|d| d.scope == self.scope)
            .ok_or_else(|| Error::UnknownDomain(name.into()))
    }

    fn key(&self, domain: &str, key: &str) -> Result<(&'static Domain, &'static schema::Key)> {
        let d = self.domain(domain)?;
        let k = d
            .key(key)
            .ok_or_else(|| Error::UnknownKey(domain.into(), key.into()))?;
        Ok((d, k))
    }

    /// The value of `domain.key`: as set, or the default.
    ///
    /// # Errors
    /// Unknown domain or key.
    pub fn get(&self, domain: &str, key: &str) -> Result<Value> {
        let (d, k) = self.key(domain, key)?;
        Ok(self.data[d.name]
            .set
            .get(k.name)
            .cloned()
            .unwrap_or_else(|| k.default.value()))
    }

    /// Every key of `domain` with its value, in schema order.
    ///
    /// # Errors
    /// Unknown domain.
    pub fn all(&self, domain: &str) -> Result<Vec<(&'static str, Value)>> {
        let d = self.domain(domain)?;
        Ok(d.keys
            .iter()
            .map(|k| {
                (
                    k.name,
                    self.data[d.name]
                        .set
                        .get(k.name)
                        .cloned()
                        .unwrap_or_else(|| k.default.value()),
                )
            })
            .collect())
    }

    /// Reads `domain`'s file again (another program may have written it:
    /// spk sets the boot default). Returns the keys whose value changed.
    ///
    /// # Errors
    /// Unknown domain.
    pub fn reload(&mut self, domain: &str) -> Result<(Vec<&'static str>, Vec<String>)> {
        let d = self.domain(domain)?;
        let before = self.all(domain)?;
        let (data, warnings) = load(&self.path(domain), d);
        self.data.insert(d.name, data);
        let after = self.all(domain)?;
        let changed = before
            .iter()
            .zip(&after)
            .filter(|(a, b)| a.1 != b.1)
            .map(|(a, _)| a.0)
            .collect();
        Ok((changed, warnings))
    }

    /// Sets `domain.key` and saves the domain; then `apply`, if given. Returns
    /// whether the value changed.
    ///
    /// # Errors
    /// Unknown domain or key, a value the key does not accept, a write
    /// failure, or `apply` failing (the old file is restored).
    pub fn set(
        &mut self,
        domain: &str,
        key: &str,
        value: Value,
        apply: Option<Apply<'_>>,
    ) -> Result<bool> {
        let (_, k) = self.key(domain, key)?;
        k.kind
            .check(&value)
            .map_err(|e| Error::Invalid(format!("{domain}.{key}: {e}")))?;
        self.change(domain, k.name, Some(value), apply)
    }

    /// Returns `domain.key` to its default (removing it from the file).
    ///
    /// # Errors
    /// As [`Store::set`].
    pub fn reset(&mut self, domain: &str, key: &str, apply: Option<Apply<'_>>) -> Result<bool> {
        let (_, k) = self.key(domain, key)?;
        self.change(domain, k.name, None, apply)
    }

    fn change(
        &mut self,
        domain: &str,
        key: &'static str,
        value: Option<Value>,
        apply: Option<Apply<'_>>,
    ) -> Result<bool> {
        let d = self.domain(domain)?;
        let before = self.get(domain, key)?;
        let old = self.data[d.name].clone();
        let entry = self.data.get_mut(d.name).expect("loaded at open");
        match value {
            Some(v) => {
                entry.set.insert(key, v);
            }
            None => {
                entry.set.remove(key);
            }
        }
        let path = self.path(domain);
        let previous = match fs::read(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => {
                self.data.insert(d.name, old);
                return Err(Error::Io(format!("{}: {e}", path.display())));
            }
        };
        let text = render(d, &self.data[d.name]);
        if let Err(e) = write_atomic(&path, text.as_bytes()) {
            self.data.insert(d.name, old);
            return Err(Error::Io(format!("{}: {e}", path.display())));
        }
        if let Some(apply) = apply
            && let Err(e) = apply(&path)
        {
            self.data.insert(d.name, old);
            let restored = match &previous {
                Some(b) => write_atomic(&path, b),
                None => fs::remove_file(&path),
            };
            return Err(Error::Apply(match restored {
                Ok(()) => e,
                Err(r) => format!("{e} (and restoring {} failed: {r})", path.display()),
            }));
        }
        Ok(self.get(domain, key)? != before)
    }

    /// The file contents for `domain` as they would be written.
    ///
    /// # Errors
    /// Unknown domain.
    pub fn render(&self, domain: &str) -> Result<String> {
        let d = self.domain(domain)?;
        Ok(render(d, &self.data[d.name]))
    }
}

fn load(path: &Path, d: &'static Domain) -> (Data, Vec<String>) {
    let mut data = Data::default();
    let mut warnings = Vec::new();
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (data, warnings),
        Err(e) => {
            warnings.push(format!("{}: {e}; using defaults", path.display()));
            return (data, warnings);
        }
    };
    let table: toml::Table = match toml::from_str(&text) {
        Ok(t) => t,
        Err(e) => {
            warnings.push(format!("{}: {e}; using defaults", path.display()));
            return (data, warnings);
        }
    };
    for (name, raw) in table {
        let Some(k) = d.key(&name) else {
            data.extra.insert(name, raw);
            continue;
        };
        let checked = Value::from_toml(&raw)
            .ok_or_else(|| "not a bool, integer, text or list of text".to_owned())
            .and_then(|v| k.kind.check(&v).map(|()| v));
        match checked {
            Ok(v) => {
                data.set.insert(k.name, v);
            }
            Err(e) => warnings.push(format!(
                "{}: {name}: {e}; using the default",
                path.display()
            )),
        }
    }
    (data, warnings)
}

fn render(d: &Domain, data: &Data) -> String {
    let mut t = toml::Table::new();
    for k in d.keys {
        let Some(v) = data.set.get(k.name) else {
            continue;
        };
        // An empty optional text means "not set": bootgen reads a missing
        // `default` as "the newest system", but refuses an empty id.
        if matches!(k.kind, Kind::Text { optional: true, .. }) && *v == Value::Text(String::new()) {
            continue;
        }
        t.insert(k.name.into(), v.to_toml());
    }
    for (k, v) in &data.extra {
        t.insert(k.clone(), v.clone());
    }
    let body = toml::to_string(&t).expect("strings, integers, bools and arrays serialize");
    format!(
        "# {} settings, written by fern-settingsd. Change them with fern-settings or Settings.\n{body}",
        d.name
    )
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("settings")
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(scope: Scope) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let (s, w) = Store::open(scope, dir.path());
        assert!(w.is_empty(), "{w:?}");
        (dir, s)
    }

    #[test]
    fn defaults_then_set_and_reset() {
        let (dir, mut s) = store(Scope::User);
        assert_eq!(
            s.get("dock", "position").unwrap(),
            Value::Text("bottom".into())
        );
        assert!(!dir.path().join("dock.toml").exists());

        assert!(
            s.set("dock", "position", Value::Text("left".into()), None)
                .unwrap()
        );
        assert!(
            !s.set("dock", "position", Value::Text("left".into()), None)
                .unwrap()
        );
        let text = fs::read_to_string(dir.path().join("dock.toml")).unwrap();
        assert!(text.contains("position = \"left\""), "{text}");

        let (again, w) = Store::open(Scope::User, dir.path());
        assert!(w.is_empty());
        assert_eq!(
            again.get("dock", "position").unwrap(),
            Value::Text("left".into())
        );

        assert!(s.reset("dock", "position", None).unwrap());
        assert_eq!(
            s.get("dock", "position").unwrap(),
            Value::Text("bottom".into())
        );
        assert!(
            !fs::read_to_string(dir.path().join("dock.toml"))
                .unwrap()
                .contains("position")
        );
    }

    #[test]
    fn refuses_bad_values_and_names() {
        let (dir, mut s) = store(Scope::User);
        let e = s
            .set("dock", "icon_size", Value::Int(500), None)
            .unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{e}");
        assert!(matches!(
            s.set("dock", "icon_size", Value::Text("big".into()), None),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(s.get("dock", "nope"), Err(Error::UnknownKey(..))));
        assert!(matches!(s.get("nope", "x"), Err(Error::UnknownDomain(_))));
        // The system's domains are not the user's.
        assert!(matches!(
            s.get("boot", "menu"),
            Err(Error::UnknownDomain(_))
        ));
        assert!(!dir.path().join("dock.toml").exists());
    }

    #[test]
    fn bad_files_fall_back_to_defaults_and_keep_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("dock.toml"),
            "position = \"top\"\nicon_size = 64\nfuture_key = [1, 2]\n",
        )
        .unwrap();
        fs::write(dir.path().join("menubar.toml"), "this is not toml").unwrap();
        let (mut s, w) = Store::open(Scope::User, dir.path());
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(
            s.get("dock", "position").unwrap(),
            Value::Text("bottom".into())
        );
        assert_eq!(s.get("dock", "icon_size").unwrap(), Value::Int(64));
        assert_eq!(s.get("menubar", "translucent").unwrap(), Value::Bool(true));

        s.set("dock", "autohide", Value::Bool(true), None).unwrap();
        let text = fs::read_to_string(dir.path().join("dock.toml")).unwrap();
        assert!(text.contains("future_key = [1, 2]"), "{text}");
        assert!(text.contains("autohide = true"), "{text}");
        assert!(!text.contains("top"), "{text}");
    }

    #[test]
    fn failed_apply_restores_the_file() {
        let (dir, mut s) = store(Scope::System);
        let path = dir.path().join("boot.toml");
        fs::write(&path, "menu = \"always\"\n").unwrap();
        s.reload("boot").unwrap();

        let mut fail = |p: &Path| -> std::result::Result<(), String> {
            let t = fs::read_to_string(p).unwrap();
            assert!(t.contains("timeout = 9") || t.contains("keep = 4"), "{t}");
            Err("the ESP is full".into())
        };
        let e = s
            .set("boot", "timeout", Value::Int(9), Some(&mut fail))
            .unwrap_err();
        assert_eq!(e, Error::Apply("the ESP is full".into()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "menu = \"always\"\n");
        assert_eq!(s.get("boot", "timeout").unwrap(), Value::Int(5));

        // With no file before, a failed apply removes it again.
        fs::remove_file(&path).unwrap();
        s.reload("boot").unwrap();
        assert!(
            s.set("boot", "keep", Value::Int(4), Some(&mut fail))
                .is_err()
        );
        assert!(!path.exists());

        let mut calls = 0;
        let mut ok = |_: &Path| -> std::result::Result<(), String> {
            calls += 1;
            Ok(())
        };
        assert!(s.set("boot", "keep", Value::Int(4), Some(&mut ok)).unwrap());
        assert_eq!(calls, 1);
    }

    #[test]
    fn reload_reports_outside_changes() {
        let (dir, mut s) = store(Scope::System);
        fs::write(dir.path().join("boot.toml"), "default = \"7\"\n").unwrap();
        let (changed, w) = s.reload("boot").unwrap();
        assert!(w.is_empty());
        assert_eq!(changed, vec!["default"]);
        assert_eq!(s.get("boot", "default").unwrap(), Value::Text("7".into()));
    }

    /// The boot domain's file is what fern-bootgen reads (through spk).
    #[test]
    fn boot_file_is_bootgen_settings() {
        let (dir, mut s) = store(Scope::System);
        let path = dir.path().join("boot.toml");
        s.set("boot", "menu", Value::Text("always".into()), None)
            .unwrap();
        s.set("boot", "timeout", Value::Int(3), None).unwrap();
        s.set(
            "boot",
            "console",
            Value::List(vec!["tty0".into(), "ttyS0,115200".into()]),
            None,
        )
        .unwrap();
        // Empty means "the newest": not written, so bootgen accepts it.
        s.set("boot", "default", Value::Text(String::new()), None)
            .unwrap();
        let b: bootgen::Settings = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(b.menu, bootgen::MenuStyle::Always);
        assert_eq!(b.timeout, 3);
        assert_eq!(b.default, None);
        assert_eq!(b.console, vec!["tty0", "ttyS0,115200"]);

        s.set("boot", "default", Value::Text("12".into()), None)
            .unwrap();
        s.set("boot", "keep", Value::Int(5), None).unwrap();
        s.set("boot", "verbose", Value::Bool(true), None).unwrap();
        s.set("boot", "theme", Value::Bool(false), None).unwrap();
        s.set("boot", "other_os", Value::Bool(false), None).unwrap();
        let b: bootgen::Settings = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(b.default.as_deref(), Some("12"));
        assert_eq!(
            (b.keep, b.verbose, b.theme, b.other_os),
            (5, true, false, false)
        );

        // And every default matches bootgen's own.
        let d = bootgen::Settings::default();
        let (fresh, _) = Store::open(Scope::System, tempfile::tempdir().unwrap().path());
        assert_eq!(
            fresh.get("boot", "timeout").unwrap(),
            Value::Int(i64::from(d.timeout))
        );
        assert_eq!(
            fresh.get("boot", "keep").unwrap(),
            Value::Int(i64::try_from(d.keep).unwrap())
        );
        assert_eq!(
            fresh.get("boot", "menu").unwrap(),
            Value::Text("hidden".into())
        );
        assert_eq!(d.menu, bootgen::MenuStyle::Hidden);
        assert_eq!(fresh.get("boot", "theme").unwrap(), Value::Bool(d.theme));
        assert_eq!(
            fresh.get("boot", "other_os").unwrap(),
            Value::Bool(d.other_os)
        );
        assert_eq!(
            fresh.get("boot", "verbose").unwrap(),
            Value::Bool(d.verbose)
        );
    }
}
