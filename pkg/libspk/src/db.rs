//! The installed-package database: `<root>/usr/lib/spk/db.sqlite`.
//!
//! It lives under `usr/` so every system snapshot carries the database that
//! describes it (DECISIONS.md, "spk database location").

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, IoContext, Result};
use crate::manifest::{FileKind, Manifest};

pub const DB_PATH: &str = "usr/lib/spk/db.sqlite";
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS packages (
    name         TEXT PRIMARY KEY,
    version      TEXT NOT NULL,
    release      INTEGER NOT NULL,
    explicit     INTEGER NOT NULL,
    installed_at INTEGER NOT NULL,
    manifest     TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
    package TEXT NOT NULL REFERENCES packages(name) ON DELETE CASCADE,
    path    TEXT NOT NULL,
    kind    TEXT NOT NULL,
    PRIMARY KEY (package, path)
);
CREATE INDEX IF NOT EXISTS files_by_path ON files(path);
CREATE TABLE IF NOT EXISTS history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    ts           INTEGER NOT NULL,
    action       TEXT NOT NULL,
    package      TEXT NOT NULL,
    old_version  TEXT,
    new_version  TEXT
);
";

#[derive(Clone, Debug)]
pub struct Installed {
    pub manifest: Manifest,
    /// Installed because the user asked for it (not only as a dependency).
    pub explicit: bool,
    pub installed_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: i64,
    pub ts: i64,
    pub action: String,
    pub package: String,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
}

#[derive(Debug)]
pub struct Db {
    conn: Connection,
    path: PathBuf,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Db {
    /// Opens (creating if needed) the database of `root`.
    ///
    /// # Errors
    /// I/O and SQLite errors, or a schema newer than this spk understands.
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join(DB_PATH);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).at(dir)?;
        }
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "DELETE")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.execute_batch(SCHEMA)?;
        let v: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
                r.get(0)
            })
            .optional()?;
        match v.as_deref().map(str::parse::<i64>) {
            None => {
                conn.execute(
                    "INSERT INTO meta(key, value) VALUES ('schema', ?1)",
                    [SCHEMA_VERSION.to_string()],
                )?;
            }
            Some(Ok(SCHEMA_VERSION)) => {}
            Some(other) => {
                return Err(Error::invalid(format!(
                    "{}: unsupported database schema {other:?}",
                    path.display()
                )));
            }
        }
        Ok(Db { conn, path })
    }

    /// Opens the database of `root` for reading only: what queries use, and
    /// the only way to read a sealed (read-only) system's database.
    ///
    /// # Errors
    /// A missing database, SQLite errors, or an unsupported schema.
    pub fn open_read_only(root: &Path) -> Result<Self> {
        let path = root.join(DB_PATH);
        let conn = Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let v: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
                r.get(0)
            })
            .optional()?;
        if v.as_deref().map(str::parse::<i64>) != Some(Ok(SCHEMA_VERSION)) {
            return Err(Error::invalid(format!(
                "{}: unsupported database schema {v:?}",
                path.display()
            )));
        }
        Ok(Db { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn row_to_installed(text: &str, explicit: bool, at: i64) -> Result<Installed> {
        Ok(Installed {
            manifest: Manifest::parse(text)?,
            explicit,
            installed_at: at,
        })
    }

    /// # Errors
    /// SQLite errors or a corrupt stored manifest.
    pub fn installed(&self) -> Result<Vec<Installed>> {
        let mut st = self
            .conn
            .prepare("SELECT manifest, explicit, installed_at FROM packages ORDER BY name")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        rows.map(|r| {
            let (t, e, a) = r?;
            Self::row_to_installed(&t, e, a)
        })
        .collect()
    }

    /// # Errors
    /// SQLite errors or a corrupt stored manifest.
    pub fn get(&self, name: &str) -> Result<Option<Installed>> {
        let row = self
            .conn
            .query_row(
                "SELECT manifest, explicit, installed_at FROM packages WHERE name=?1",
                [name],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, bool>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(t, e, a)| Self::row_to_installed(&t, e, a))
            .transpose()
    }

    /// Packages owning `path` (directories can have several owners).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn owners(&self, path: &str) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT package FROM files WHERE path=?1 ORDER BY package")?;
        let rows = st.query_map([path], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Non-directory owners of `path` other than `except`.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn file_owner_other_than(&self, path: &str, except: &[&str]) -> Result<Option<String>> {
        let mut st = self
            .conn
            .prepare("SELECT package FROM files WHERE path=?1 AND kind != 'dir'")?;
        let rows = st.query_map([path], |r| r.get::<_, String>(0))?;
        for r in rows {
            let owner = r?;
            if !except.contains(&owner.as_str()) {
                return Ok(Some(owner));
            }
        }
        Ok(None)
    }

    /// Starts a write transaction. Nothing is visible until
    /// [`DbTx::commit`].
    ///
    /// # Errors
    /// SQLite errors.
    pub fn begin(&mut self) -> Result<DbTx<'_>> {
        Ok(DbTx {
            tx: self.conn.transaction()?,
        })
    }

    /// # Errors
    /// SQLite errors.
    pub fn history(&self) -> Result<Vec<HistoryEntry>> {
        let mut st = self.conn.prepare(
            "SELECT id, ts, action, package, old_version, new_version FROM history ORDER BY id",
        )?;
        let rows = st.query_map([], |r| {
            Ok(HistoryEntry {
                id: r.get(0)?,
                ts: r.get(1)?,
                action: r.get(2)?,
                package: r.get(3)?,
                old_version: r.get(4)?,
                new_version: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[derive(Debug)]
pub struct DbTx<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl DbTx<'_> {
    /// Records `m` as installed, replacing any previous version of it.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn put_package(&self, m: &Manifest, explicit: bool, ts: i64) -> Result<()> {
        self.tx
            .execute("DELETE FROM packages WHERE name=?1", [&m.name])?;
        self.tx.execute(
            "INSERT INTO packages(name, version, release, explicit, installed_at, manifest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                m.name,
                m.version.as_str(),
                m.release,
                explicit,
                ts,
                m.to_toml()?
            ],
        )?;
        let mut st = self
            .tx
            .prepare("INSERT INTO files(package, path, kind) VALUES (?1, ?2, ?3)")?;
        for f in &m.files {
            st.execute(params![m.name, f.path, f.kind.to_string()])?;
        }
        Ok(())
    }

    /// # Errors
    /// SQLite errors.
    pub fn remove_package(&self, name: &str) -> Result<()> {
        self.tx
            .execute("DELETE FROM packages WHERE name=?1", [name])?;
        Ok(())
    }

    /// # Errors
    /// SQLite errors.
    pub fn set_explicit(&self, name: &str, explicit: bool) -> Result<()> {
        self.tx.execute(
            "UPDATE packages SET explicit=?2 WHERE name=?1",
            params![name, explicit],
        )?;
        Ok(())
    }

    /// # Errors
    /// SQLite errors.
    pub fn record(
        &self,
        ts: i64,
        action: &str,
        package: &str,
        old: Option<&str>,
        new: Option<&str>,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT INTO history(ts, action, package, old_version, new_version)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![ts, action, package, old, new],
        )?;
        Ok(())
    }

    /// # Errors
    /// SQLite errors.
    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }
}

/// Paths of `m` that are not directories.
pub fn non_dir_paths(m: &Manifest) -> impl Iterator<Item = &str> {
    m.files
        .iter()
        .filter(|f| f.kind != FileKind::Dir)
        .map(|f| f.path.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::tests::sample;

    #[test]
    fn install_query_remove() {
        let root = tempfile::tempdir().unwrap();
        let mut db = Db::open(root.path()).unwrap();
        assert!(db.installed().unwrap().is_empty());

        let m = sample();
        let tx = db.begin().unwrap();
        tx.put_package(&m, true, 42).unwrap();
        tx.record(42, "install", "demo", None, Some("1.0-1"))
            .unwrap();
        tx.commit().unwrap();

        let got = db.get("demo").unwrap().unwrap();
        assert_eq!(got.manifest, m);
        assert!(got.explicit);
        assert_eq!(db.owners("usr/bin/demo").unwrap(), ["demo"]);
        assert_eq!(db.owners("usr/bin").unwrap(), ["demo"]);
        assert_eq!(
            db.file_owner_other_than("usr/bin/demo", &[])
                .unwrap()
                .as_deref(),
            Some("demo")
        );
        assert_eq!(
            db.file_owner_other_than("usr/bin/demo", &["demo"]).unwrap(),
            None
        );
        // Directories never count as conflicting files.
        assert_eq!(db.file_owner_other_than("usr/bin", &[]).unwrap(), None);

        let tx = db.begin().unwrap();
        tx.remove_package("demo").unwrap();
        tx.commit().unwrap();
        assert!(db.get("demo").unwrap().is_none());
        assert!(
            db.owners("usr/bin/demo").unwrap().is_empty(),
            "files cascade"
        );
        assert_eq!(db.history().unwrap().len(), 1);
    }

    #[test]
    fn uncommitted_transaction_rolls_back() {
        let root = tempfile::tempdir().unwrap();
        let mut db = Db::open(root.path()).unwrap();
        {
            let tx = db.begin().unwrap();
            tx.put_package(&sample(), true, 1).unwrap();
            // dropped without commit
        }
        assert!(db.get("demo").unwrap().is_none());
    }

    #[test]
    fn read_only_open_reads_and_never_writes() {
        let root = tempfile::tempdir().unwrap();
        assert!(Db::open_read_only(root.path()).is_err(), "no database yet");
        {
            let mut db = Db::open(root.path()).unwrap();
            let tx = db.begin().unwrap();
            tx.put_package(&sample(), true, 1).unwrap();
            tx.commit().unwrap();
        }
        let ro = Db::open_read_only(root.path()).unwrap();
        assert!(ro.get("demo").unwrap().unwrap().explicit);
        assert!(
            ro.conn.execute("DELETE FROM meta", []).is_err(),
            "read-only connection wrote"
        );
        drop(ro);
        let db = Db::open(root.path()).unwrap();
        db.conn
            .execute("UPDATE meta SET value='99' WHERE key='schema'", [])
            .unwrap();
        drop(db);
        assert!(Db::open_read_only(root.path()).is_err());
    }

    #[test]
    fn reopen_persists_and_rejects_future_schema() {
        let root = tempfile::tempdir().unwrap();
        {
            let mut db = Db::open(root.path()).unwrap();
            let tx = db.begin().unwrap();
            tx.put_package(&sample(), false, 1).unwrap();
            tx.commit().unwrap();
        }
        let db = Db::open(root.path()).unwrap();
        assert!(!db.get("demo").unwrap().unwrap().explicit);
        db.conn
            .execute("UPDATE meta SET value='99' WHERE key='schema'", [])
            .unwrap();
        drop(db);
        assert!(Db::open(root.path()).is_err());
    }

    #[test]
    fn upgrade_replaces_file_list() {
        let root = tempfile::tempdir().unwrap();
        let mut db = Db::open(root.path()).unwrap();
        let m1 = sample();
        let mut m2 = sample();
        m2.release = 2;
        m2.files.retain(|f| f.path != "usr/bin/demo2");
        let tx = db.begin().unwrap();
        tx.put_package(&m1, true, 1).unwrap();
        tx.put_package(&m2, true, 2).unwrap();
        tx.commit().unwrap();
        assert!(db.owners("usr/bin/demo2").unwrap().is_empty());
        assert_eq!(db.get("demo").unwrap().unwrap().manifest.release, 2);
    }
}
