//! Repository invariant checks for Fern.
//!
//! These enforce ground rules from `CLAUDE.md` that a compiler cannot:
//!
//! * no unfilled name placeholders anywhere in the tree,
//! * no Apple trademarked names or Apple font files in shipped sources
//!   and assets (rule 9),
//! * every recipe in `recipes/` has a one-line justification in
//!   `docs/DECISIONS.md`, and every justification names a real recipe
//!   (section 6.3),
//! * the documents every session depends on exist.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Files every session reads or writes; their absence is a scaffold error.
pub const REQUIRED_FILES: &[&str] = &[
    "CLAUDE.md",
    "justfile",
    "flake.nix",
    "docs/STATUS.md",
    "docs/DECISIONS.md",
    "docs/design/HIG.md",
];

/// Directories that never contain project sources.
const SKIP_DIRS: &[&str] = &[
    ".git", "target", "result", ".direnv", "build", "out", ".ccache",
];

/// Names from section 0 of the brief. Kept as bare words so this file
/// does not itself contain the placeholder pattern it searches for.
const PLACEHOLDER_NAMES: &[&str] = &["OS", "PFX", "PKG", "SH", "DE"];

/// Trademarked names that must not ship. Matched case-sensitively on word
/// boundaries.
const FORBIDDEN_NAMES: &[&str] = &[
    "Apple",
    "macOS",
    "Mac OS",
    "Finder",
    "Spotlight",
    "Mission Control",
    "Launchpad",
    "Cupertino",
    "SF Pro",
    "SF Mono",
    "SF Compact",
    "SF Symbols",
    "San Francisco",
];

/// Top-level paths that ship to users and are scanned for forbidden
/// names. Docs and the brief legitimately talk about what we avoid.
const SHIPPED_ROOTS: &[&str] = &[
    "apps",
    "assets",
    "boot",
    "bootstrap",
    "de",
    "image",
    "init",
    "pkg",
    "recipes",
    "sh",
];

/// Lines containing this marker are exempt from the forbidden-name check
/// (for example a compatibility shim that must mention a foreign format).
pub const ALLOW_MARKER: &str = "ferncheck: allow-name";

/// Heading in `docs/DECISIONS.md` under which recipe justifications live.
pub const JUSTIFICATION_HEADING: &str = "## Package justifications";

const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub check: &'static str,
    pub path: PathBuf,
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.check, self.path.display())?;
        if let Some(line) = self.line {
            write!(f, ":{line}")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// Runs every check against the repository at `root`.
///
/// # Errors
/// Returns an I/O error if the tree cannot be walked.
pub fn run_all(root: &Path) -> io::Result<Vec<Violation>> {
    let files = walk(root)?;
    let mut out = check_required_files(root);
    out.extend(check_placeholders(root, &files));
    out.extend(check_forbidden_names(root, &files));
    out.extend(check_recipe_justifications(root)?);
    Ok(out)
}

/// Returns every regular file under `root`, relative to it, sorted.
///
/// # Errors
/// Returns an I/O error if a directory cannot be read.
pub fn walk(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for entry in fs::read_dir(root.join(&rel))? {
            let entry = entry?;
            let name = entry.file_name();
            let child = rel.join(&name);
            let ty = entry.file_type()?;
            if ty.is_dir() {
                if !SKIP_DIRS.iter().any(|s| name == *s) {
                    stack.push(child);
                }
            } else if ty.is_file() {
                out.push(child);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn read_text(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_TEXT_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub fn check_required_files(root: &Path) -> Vec<Violation> {
    REQUIRED_FILES
        .iter()
        .filter(|f| !root.join(f).is_file())
        .map(|f| Violation {
            check: "required",
            path: PathBuf::from(f),
            line: None,
            message: "required file is missing".into(),
        })
        .collect()
}

pub fn check_placeholders(root: &Path, files: &[PathBuf]) -> Vec<Violation> {
    let patterns: Vec<String> = PLACEHOLDER_NAMES
        .iter()
        .map(|n| format!("{{{{{n}}}}}"))
        .collect();
    let mut out = Vec::new();
    for rel in files {
        let Some(text) = read_text(&root.join(rel)) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            for p in &patterns {
                if line.contains(p.as_str()) {
                    out.push(Violation {
                        check: "placeholder",
                        path: rel.clone(),
                        line: Some(i + 1),
                        message: format!("unfilled name placeholder {p}"),
                    });
                }
            }
        }
    }
    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Finds `needle` in `hay` on word boundaries.
fn contains_word(hay: &str, needle: &str) -> bool {
    let bytes = hay.as_bytes();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(needle) {
        let at = start + pos;
        let end = at + needle.len();
        let before_ok = at == 0 || !is_word_byte(bytes[at - 1]);
        let after_ok = end == bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        start = at + 1;
    }
    false
}

/// Font and icon files that are Apple's by name or format.
fn is_apple_asset_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let font = matches!(ext, "otf" | "ttf" | "ttc" | "woff" | "woff2");
    let apple_font = font
        && ["sf-", "sfpro", "sfmono", "sfcompact", "sfns", "newyork"]
            .iter()
            .any(|p| lower.starts_with(p));
    apple_font || matches!(ext, "icns" | "car")
}

fn is_shipped(rel: &Path) -> bool {
    rel.components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .is_some_and(|first| SHIPPED_ROOTS.contains(&first))
}

pub fn check_forbidden_names(root: &Path, files: &[PathBuf]) -> Vec<Violation> {
    let mut out = Vec::new();
    for rel in files {
        if let Some(name) = rel.file_name().and_then(|n| n.to_str())
            && is_apple_asset_name(name)
        {
            out.push(Violation {
                check: "apple-asset",
                path: rel.clone(),
                line: None,
                message: "Apple font or icon file; use Inter, JetBrains Mono, or our own art"
                    .into(),
            });
        }
        if !is_shipped(rel) {
            continue;
        }
        let Some(text) = read_text(&root.join(rel)) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line.contains(ALLOW_MARKER) {
                continue;
            }
            for name in FORBIDDEN_NAMES {
                if contains_word(line, name) {
                    out.push(Violation {
                        check: "apple-name",
                        path: rel.clone(),
                        line: Some(i + 1),
                        message: format!("trademarked name \"{name}\" in shipped file"),
                    });
                }
            }
        }
    }
    out
}

/// Parses `- <name>: <reason>` lines (name optionally in backticks) under
/// [`JUSTIFICATION_HEADING`] until the next `## ` heading.
pub fn parse_justifications(decisions: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in decisions.lines() {
        if line.starts_with("## ") {
            inside = line.trim_end() == JUSTIFICATION_HEADING;
            continue;
        }
        if !inside {
            continue;
        }
        let Some(item) = line.strip_prefix("- ") else {
            continue;
        };
        let Some((name, reason)) = item.split_once(':') else {
            continue;
        };
        let name = name.trim().trim_matches('`').trim();
        if !name.is_empty() {
            out.push((name.to_string(), reason.trim().to_string()));
        }
    }
    out
}

/// # Errors
/// Returns an I/O error if `recipes/` exists but cannot be read.
pub fn check_recipe_justifications(root: &Path) -> io::Result<Vec<Violation>> {
    let mut recipes = Vec::new();
    let dir = root.join("recipes");
    if dir.is_dir() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() && entry.path().join("recipe.toml").is_file() {
                recipes.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    recipes.sort();

    let decisions_path = PathBuf::from("docs/DECISIONS.md");
    let decisions = fs::read_to_string(root.join(&decisions_path)).unwrap_or_default();
    let justified = parse_justifications(&decisions);

    let mut out = Vec::new();
    for r in &recipes {
        match justified.iter().find(|(n, _)| n == r) {
            None => out.push(Violation {
                check: "justification",
                path: PathBuf::from("recipes").join(r),
                line: None,
                message: format!("no entry for `{r}` under \"{JUSTIFICATION_HEADING}\""),
            }),
            Some((_, reason)) if reason.is_empty() => out.push(Violation {
                check: "justification",
                path: decisions_path.clone(),
                line: None,
                message: format!("justification for `{r}` is empty"),
            }),
            Some(_) => {}
        }
    }
    for (name, _) in &justified {
        if !recipes.contains(name) {
            out.push(Violation {
                check: "justification",
                path: decisions_path.clone(),
                line: None,
                message: format!(
                    "justification for `{name}`, but recipes/{name}/recipe.toml does not exist"
                ),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "ferncheck-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            for f in REQUIRED_FILES {
                let p = dir.join(f);
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(&p, "").unwrap();
            }
            TempRepo(dir)
        }

        fn write(&self, rel: &str, contents: &str) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, contents).unwrap();
        }

        fn checks(&self) -> Vec<&'static str> {
            run_all(&self.0)
                .unwrap()
                .into_iter()
                .map(|v| v.check)
                .collect()
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn clean_repo_passes() {
        let repo = TempRepo::new();
        assert!(repo.checks().is_empty());
    }

    #[test]
    fn missing_required_file() {
        let repo = TempRepo::new();
        fs::remove_file(repo.0.join("docs/STATUS.md")).unwrap();
        assert_eq!(repo.checks(), vec!["required"]);
    }

    #[test]
    fn placeholder_detected_anywhere() {
        let repo = TempRepo::new();
        repo.write(
            "docs/notes.md",
            &format!("binary is {{{{{}}}}}-init\n", "PFX"),
        );
        let v = run_all(&repo.0).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "placeholder");
        assert_eq!(v[0].line, Some(1));
    }

    #[test]
    fn forbidden_name_only_in_shipped_paths() {
        let repo = TempRepo::new();
        repo.write("docs/notes.md", "Files, not Finder.\n");
        assert!(repo.checks().is_empty());
        repo.write("apps/files/Main.qml", "title: \"Finder\"\n");
        assert_eq!(repo.checks(), vec!["apple-name"]);
    }

    #[test]
    fn forbidden_name_respects_word_boundaries_and_marker() {
        let repo = TempRepo::new();
        repo.write(
            "de/shell/a.qml",
            "property var pineapple: 1\nPathFinder {}\n",
        );
        assert!(repo.checks().is_empty());
        repo.write(
            "de/shell/b.rs",
            "// reads Apple .DS_Store files  ferncheck: allow-name\n",
        );
        assert!(repo.checks().is_empty());
    }

    #[test]
    fn apple_font_files_rejected_everywhere() {
        let repo = TempRepo::new();
        repo.write("assets/fonts/Inter-Regular.ttf", "x");
        assert!(repo.checks().is_empty());
        repo.write("docs/SF-Pro-Display-Regular.otf", "x");
        repo.write("assets/icons/app.icns", "x");
        assert_eq!(repo.checks(), vec!["apple-asset", "apple-asset"]);
    }

    #[test]
    fn binary_files_are_skipped() {
        let repo = TempRepo::new();
        repo.write("assets/blob.bin", "\0Finder\0");
        assert!(repo.checks().is_empty());
    }

    #[test]
    fn recipes_need_justification() {
        let repo = TempRepo::new();
        repo.write("recipes/zstd/recipe.toml", "name = \"zstd\"\n");
        assert_eq!(repo.checks(), vec!["justification"]);

        repo.write(
            "docs/DECISIONS.md",
            "# Decisions\n\n## Package justifications\n\n- `zstd`: package compression\n\n## Log\n- `other`: not a justification\n",
        );
        assert!(repo.checks().is_empty());
    }

    #[test]
    fn empty_or_stale_justification_rejected() {
        let repo = TempRepo::new();
        repo.write("recipes/zstd/recipe.toml", "");
        repo.write(
            "docs/DECISIONS.md",
            "## Package justifications\n- zstd:\n- `gone`: removed long ago\n",
        );
        let v = run_all(&repo.0).unwrap();
        assert_eq!(v.len(), 2);
        assert!(v[0].message.contains("empty"));
        assert!(v[1].message.contains("gone"));
    }

    #[test]
    fn skip_dirs_are_not_walked() {
        let repo = TempRepo::new();
        repo.write("target/debug/x.txt", &format!("{{{{{}}}}}", "OS"));
        assert!(repo.checks().is_empty());
    }

    #[test]
    fn parse_justifications_stops_at_next_heading() {
        let text = "## Package justifications\n- a: one\n- `b` : two\n## Other\n- c: three\n";
        assert_eq!(
            parse_justifications(text),
            vec![("a".into(), "one".into()), ("b".into(), "two".into())]
        );
    }
}
