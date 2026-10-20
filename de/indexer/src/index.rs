//! The index: one row per file or folder (path, name, folder or not), and
//! an FTS5 table over the names for prefix search.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

/// One search result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    pub name: String,
    pub folder: bool,
}

/// The index database.
#[derive(Debug)]
pub struct Index {
    db: Connection,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    folder INTEGER NOT NULL,
    seen INTEGER NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS names USING fts5(
    name, content = 'files', content_rowid = 'id',
    tokenize = \"unicode61 remove_diacritics 2 tokenchars '_'\"
);
CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
    INSERT INTO names(rowid, name) VALUES (new.id, new.name);
END;
CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
    INSERT INTO names(names, rowid, name) VALUES ('delete', old.id, old.name);
END;
CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE OF name ON files BEGIN
    INSERT INTO names(names, rowid, name) VALUES ('delete', old.id, old.name);
    INSERT INTO names(rowid, name) VALUES (new.id, new.name);
END;
";

impl Index {
    /// Opens (or creates) the index at `path`; `:memory:` for tests.
    ///
    /// # Errors
    /// The file cannot be opened or is not an index.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(SCHEMA)?;
        Ok(Index { db })
    }

    /// Adds or refreshes one entry, marking it seen in crawl `seen`.
    ///
    /// # Errors
    /// The database refused the write.
    pub fn put(&self, path: &str, folder: bool, seen: i64) -> rusqlite::Result<()> {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.db.execute(
            "INSERT INTO files(path, name, folder, seen) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET folder = excluded.folder, seen = excluded.seen",
            params![path, name, folder, seen],
        )?;
        Ok(())
    }

    /// Removes `path` and, for a folder, everything below it.
    ///
    /// # Errors
    /// The database refused the write.
    pub fn remove(&self, path: &str) -> rusqlite::Result<usize> {
        // A prefix by length, not LIKE: names may hold % and _.
        let below = format!("{}/", path.trim_end_matches('/'));
        self.db.execute(
            "DELETE FROM files WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
            params![path, below],
        )
    }

    /// Forgets everything under `root` not seen in crawl `seen` (gone
    /// while we were not watching).
    ///
    /// # Errors
    /// The database refused the write.
    pub fn sweep(&self, root: &str, seen: i64) -> rusqlite::Result<usize> {
        let below = format!("{}/", root.trim_end_matches('/'));
        self.db.execute(
            "DELETE FROM files WHERE seen != ?1 AND substr(path, 1, length(?2)) = ?2",
            params![seen, below],
        )
    }

    /// Runs `f` in one transaction (a crawl writes thousands of rows).
    ///
    /// # Errors
    /// The transaction failed; nothing of it is kept.
    pub fn batch<T>(
        &mut self,
        f: impl FnOnce(&Index) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        self.db.execute_batch("BEGIN")?;
        match f(self) {
            Ok(v) => {
                self.db.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.db.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// How many entries there are.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn len(&self) -> rusqlite::Result<u64> {
        self.db
            .query_row("SELECT count(*) FROM files", [], |r| r.get::<_, i64>(0))
            .map(|n| u64::try_from(n).unwrap_or(0))
    }

    /// Whether nothing is indexed.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn is_empty(&self) -> rusqlite::Result<bool> {
        self.len().map(|n| n == 0)
    }

    /// Whether `path` is indexed.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn contains(&self, path: &str) -> rusqlite::Result<bool> {
        self.db
            .query_row("SELECT 1 FROM files WHERE path = ?1", [path], |_| Ok(()))
            .optional()
            .map(|r| r.is_some())
    }

    /// The best `limit` entries whose name has words starting with every
    /// word of `text`: names that start with the text first, then by
    /// relevance, shorter paths first.
    ///
    /// # Errors
    /// The database could not be read.
    pub fn search(&self, text: &str, limit: u32) -> rusqlite::Result<Vec<Hit>> {
        let Some(query) = fts_query(text) else {
            return Ok(Vec::new());
        };
        let start = format!("{}%", text.trim().to_lowercase().replace(['%', '_'], ""));
        let mut stmt = self.db.prepare_cached(
            "SELECT files.path, files.name, files.folder FROM names
             JOIN files ON files.id = names.rowid
             WHERE names MATCH ?1
             ORDER BY lower(files.name) LIKE ?2 DESC, bm25(names), length(files.path)
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![query, start, limit], |r| {
            Ok(Hit {
                path: r.get(0)?,
                name: r.get(1)?,
                folder: r.get(2)?,
            })
        })?;
        rows.collect()
    }
}

/// `text` as an FTS5 query: every word a quoted prefix ("rep"* "q3"*), so
/// nothing the user types is read as query syntax. None when no word is
/// left.
#[must_use]
pub fn fts_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .take(8)
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        let i = Index::open(Path::new(":memory:")).unwrap();
        for (p, d) in [
            ("/home/sam/Documents", true),
            ("/home/sam/Documents/Quarterly Report.pdf", false),
            ("/home/sam/Documents/report-draft.txt", false),
            ("/home/sam/Documents/Old/report 2019.odt", false),
            (
                "/home/sam/Pictures/Screenshot 2026-10-20 at 21.04.07.png",
                false,
            ),
            ("/home/sam/100%_done.txt", false),
            ("/home/sam/Café menu.pdf", false),
        ] {
            i.put(p, d, 1).unwrap();
        }
        i
    }

    fn names(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.name.as_str()).collect()
    }

    #[test]
    fn prefixes_of_words_and_starts_first() {
        let i = index();
        let hits = i.search("rep", 10).unwrap();
        assert_eq!(hits.len(), 3, "{hits:?}");
        // Names starting with it come first.
        assert!(hits[0].name.starts_with("report"), "{hits:?}");
        assert!(names(&hits).contains(&"Quarterly Report.pdf"));
        // Every word must match.
        assert_eq!(
            names(&i.search("quart rep", 10).unwrap()),
            ["Quarterly Report.pdf"]
        );
        assert_eq!(names(&i.search("2026 screen", 10).unwrap()).len(), 1);
        assert!(i.search("nothing", 10).unwrap().is_empty());
        assert_eq!(i.search("rep", 1).unwrap().len(), 1);
    }

    #[test]
    fn folders_case_and_accents() {
        let i = index();
        let hits = i.search("DOCU", 10).unwrap();
        assert_eq!(
            hits,
            [Hit {
                path: "/home/sam/Documents".into(),
                name: "Documents".into(),
                folder: true
            }]
        );
        assert_eq!(names(&i.search("cafe", 10).unwrap()), ["Café menu.pdf"]);
    }

    #[test]
    fn query_syntax_is_just_text() {
        let i = index();
        for q in [
            "\"", "*", "NOT", "rep OR", "a:b", "(", "100%", "-", "", "   ",
        ] {
            assert!(i.search(q, 10).is_ok(), "{q}");
        }
        assert_eq!(
            names(&i.search("100%_done", 10).unwrap()),
            ["100%_done.txt"]
        );
        assert_eq!(fts_query("a \"b\" c*"), Some("\"a\"* \"b\"* \"c\"*".into()));
        assert_eq!(fts_query(" -- "), None);
    }

    #[test]
    fn removing_a_folder_takes_what_is_in_it() {
        let i = index();
        assert_eq!(i.remove("/home/sam/Documents").unwrap(), 4);
        assert!(i.search("report", 10).unwrap().is_empty());
        // A sibling whose name starts the same stays.
        i.put("/home/sam/Doc", true, 1).unwrap();
        i.put("/home/sam/Doc2/x.txt", false, 1).unwrap();
        i.remove("/home/sam/Doc").unwrap();
        assert!(i.contains("/home/sam/Doc2/x.txt").unwrap());
        // Names with LIKE wildcards are not patterns.
        i.put("/home/sam/a%/b", false, 1).unwrap();
        i.put("/home/sam/ab/c", false, 1).unwrap();
        i.remove("/home/sam/a%").unwrap();
        assert!(i.contains("/home/sam/ab/c").unwrap());
        assert!(!i.contains("/home/sam/a%/b").unwrap());
    }

    #[test]
    fn sweep_forgets_what_a_crawl_did_not_see() {
        let mut i = index();
        i.batch(|i| {
            i.put("/home/sam/Documents", true, 2)?;
            i.put("/home/sam/Documents/Quarterly Report.pdf", false, 2)
        })
        .unwrap();
        assert_eq!(i.sweep("/home/sam", 2).unwrap(), 5);
        assert_eq!(i.len().unwrap(), 2);
        assert!(!i.is_empty().unwrap());
        // Outside the root: untouched.
        i.put("/other/x", false, 1).unwrap();
        i.sweep("/home/sam", 2).unwrap();
        assert!(i.contains("/other/x").unwrap());
    }

    #[test]
    fn a_failed_batch_keeps_nothing() {
        let mut i = index();
        let before = i.len().unwrap();
        let r: rusqlite::Result<()> = i.batch(|i| {
            i.put("/home/sam/new", false, 1)?;
            Err(rusqlite::Error::InvalidQuery)
        });
        assert!(r.is_err());
        assert_eq!(i.len().unwrap(), before);
    }
}
