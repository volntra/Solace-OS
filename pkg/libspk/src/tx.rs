//! Install / remove / upgrade transactions against a root directory.
//!
//! Phase 1 scope: a transaction is all-or-nothing *within one run*. Every
//! file that is replaced or removed is kept as a backup (hard link or rename
//! inside the root) until the database commit succeeds, and restored if any
//! step fails. A run that is killed mid-apply leaves its staging directory
//! behind; the next run refuses to start until it is dealt with. Crash-safe
//! system updates come from btrfs snapshots in Phase 3 (CLAUDE.md 6.2).

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::db::{self, Db, non_dir_paths};
use crate::error::{Error, IoContext, Result};
use crate::fetch::Location;
use crate::manifest::{FileKind, Manifest};
use crate::package::VerifiedPackage;
use crate::repo::Repo;
use crate::sign::Keyring;
use crate::solve::{self, Candidate, Plan, Request};
use crate::version::DepSpec;

const SPK_DIR: &str = "usr/lib/spk";
const LOCK: &str = "usr/lib/spk/lock";
const STAGING: &str = "usr/lib/spk/staging";

/// Progress notifications for the CLI (and later the daemon).
#[derive(Clone, Debug)]
pub enum Event {
    Resolving,
    Planned {
        install: Vec<String>,
        remove: Vec<String>,
    },
    Fetching {
        name: String,
        index: usize,
        total: usize,
    },
    Extracting {
        name: String,
    },
    Applying,
    Done,
}

/// An opened root: trusted keys, database, and an exclusive lock.
#[derive(Debug)]
pub struct Root {
    pub path: PathBuf,
    pub keyring: Keyring,
    pub db: Db,
    _lock: File,
}

impl Root {
    /// Opens `path` for reading and writing, creating the spk directories if
    /// needed. Holds an exclusive lock until dropped.
    ///
    /// # Errors
    /// I/O errors, a busy lock, or a malformed keyring.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_inner(path, None)
    }

    /// Like [`Root::open`], trusting `keyring` instead of the keys under
    /// `path` (a system transaction's staging root has no `/etc`; it uses
    /// the running system's keys).
    ///
    /// # Errors
    /// As [`Root::open`].
    pub fn open_with_keyring(path: &Path, keyring: Keyring) -> Result<Self> {
        Self::open_inner(path, Some(keyring))
    }

    fn open_inner(path: &Path, keyring: Option<Keyring>) -> Result<Self> {
        fs::create_dir_all(path.join(SPK_DIR)).at(path.join(SPK_DIR))?;
        let path = path.canonicalize().at(path)?;
        let lock_path = path.join(LOCK);
        let lock = File::create(&lock_path).at(&lock_path)?;
        lock.try_lock().map_err(|_| {
            Error::Conflict(format!(
                "{} is in use by another spk process",
                path.display()
            ))
        })?;
        let keyring = match keyring {
            Some(k) => k,
            None => Keyring::load(&path)?,
        };
        let db = Db::open(&path)?;
        Ok(Root {
            path,
            keyring,
            db,
            _lock: lock,
        })
    }

    fn require_keys(&self) -> Result<()> {
        if self.keyring.is_empty() {
            return Err(Error::Signature(format!(
                "{} trusts no signing keys; add one with `spk key trust <key.pub> --root {}`",
                self.path.display(),
                self.path.display()
            )));
        }
        Ok(())
    }
}

/// Where a candidate came from.
enum Origin {
    Installed,
    Repo { repo: usize, entry: usize },
    Local(usize),
}

/// Inputs to a transaction besides the request.
#[derive(Debug, Default)]
pub struct Sources {
    pub repos: Vec<Location>,
    /// Package files given directly on the command line.
    pub files: Vec<PathBuf>,
}

/// Resolves and applies `req` to `root`.
///
/// # Errors
/// Any resolution, verification, conflict, or I/O error. On error the
/// root's files and database are as they were before the call.
pub fn run(
    root: &mut Root,
    req: &Request,
    sources: &Sources,
    on: &mut dyn FnMut(Event),
) -> Result<Plan> {
    root.require_keys()?;
    let staging = root.path.join(STAGING);
    if staging.exists() {
        return Err(Error::Conflict(format!(
            "{} exists: a previous transaction was interrupted. Inspect it, then \
             remove it to continue (automatic recovery arrives with Phase 3 snapshots)",
            staging.display()
        )));
    }
    fs::create_dir_all(&staging).at(&staging)?;
    let result = run_staged(root, req, sources, &staging, on);
    // Staging holds downloads, extracted payloads, and backups; it is only
    // removed once the transaction is committed or fully rolled back.
    if result.is_ok() || !staging.join(JOURNAL_MARK).exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// Present while files in the root are being changed.
const JOURNAL_MARK: &str = "APPLYING";

fn run_staged(
    root: &mut Root,
    req: &Request,
    sources: &Sources,
    staging: &Path,
    on: &mut dyn FnMut(Event),
) -> Result<Plan> {
    let dl = staging.join("download");
    fs::create_dir(&dl).at(&dl)?;

    let u = Universe::gather(root, sources, &dl)?;
    let Universe {
        installed,
        cands,
        origins,
        repos,
        mut local,
    } = u;

    on(Event::Resolving);
    let plan = solve::solve(&cands, req)?;
    on(Event::Planned {
        install: plan
            .install
            .iter()
            .map(|i| format!("{} {}", cands[*i].name, cands[*i].version))
            .collect(),
        remove: plan.remove.clone(),
    });
    if plan.install.is_empty() && plan.remove.is_empty() {
        on(Event::Done);
        return Ok(plan);
    }

    // Fetch + verify everything before touching the root.
    let mut new_pkgs: Vec<VerifiedPackage> = Vec::new();
    for (n, i) in plan.install.iter().enumerate() {
        on(Event::Fetching {
            name: cands[*i].name.clone(),
            index: n + 1,
            total: plan.install.len(),
        });
        let vp = match origins[*i] {
            Origin::Repo { repo, entry } => {
                let r = &repos[repo];
                r.fetch_package(&r.index.packages[entry], &root.keyring, &dl)?
            }
            Origin::Local(k) => local[k]
                .take()
                .ok_or_else(|| Error::invalid("package listed twice"))?,
            Origin::Installed => unreachable!("installed candidates are never in plan.install"),
        };
        new_pkgs.push(vp);
    }

    // Extract each into its own staging dir.
    let extract_root = staging.join("extract");
    fs::create_dir(&extract_root).at(&extract_root)?;
    let mut extracted = Vec::new();
    for vp in &new_pkgs {
        on(Event::Extracting {
            name: vp.manifest.name.clone(),
        });
        let d = extract_root.join(&vp.manifest.name);
        fs::create_dir(&d).at(&d)?;
        vp.extract(&d)?;
        extracted.push(d);
    }

    let old: HashMap<String, Manifest> = installed
        .into_iter()
        .map(|i| (i.manifest.name.clone(), i.manifest))
        .collect();
    let new_manifests: Vec<&Manifest> = new_pkgs.iter().map(|v| &v.manifest).collect();
    check_conflicts(root, &old, &new_manifests, &plan.remove)?;

    on(Event::Applying);
    let mut j = Journal::new(staging)?;
    let applied = apply(root, &old, &new_manifests, &extracted, &plan.remove, &mut j)
        .and_then(|()| commit_db(root, req, &old, &new_manifests, &plan.remove));
    match applied {
        Ok(()) => {
            j.finish();
            prune_dirs(root, &old, &new_manifests, &plan.remove);
            on(Event::Done);
            Ok(plan)
        }
        Err(e) => match j.rollback() {
            Ok(()) => Err(e),
            Err(re) => Err(Error::Conflict(format!(
                "{e}; rolling back ALSO failed ({re}). Backups are kept in {}",
                staging.display()
            ))),
        },
    }
}

/// Everything the solver can choose from, and where each candidate lives.
struct Universe {
    installed: Vec<db::Installed>,
    cands: Vec<Candidate>,
    origins: Vec<Origin>,
    repos: Vec<Repo>,
    local: Vec<Option<VerifiedPackage>>,
}

impl Universe {
    /// Installed packages, every verified repo's index, and local package
    /// files (verified now). Repo candidates identical to an installed
    /// package are left out so they are never "reinstalled".
    fn gather(root: &Root, sources: &Sources, dl: &Path) -> Result<Self> {
        let installed = root.db.installed()?;
        let mut cands = Vec::new();
        let mut origins = Vec::new();
        for i in &installed {
            cands.push(candidate_of(&i.manifest, true));
            origins.push(Origin::Installed);
        }
        let have: HashSet<(String, String)> = installed
            .iter()
            .map(|i| {
                (
                    i.manifest.name.clone(),
                    i.manifest.pkg_version().to_string(),
                )
            })
            .collect();
        let mut repos = Vec::new();
        for loc in &sources.repos {
            repos.push(Repo::open(loc.clone(), &root.keyring, dl)?);
        }
        for (ri, r) in repos.iter().enumerate() {
            for (ei, e) in r.index.packages.iter().enumerate() {
                if !have.contains(&(e.name.clone(), e.pkg_version().to_string())) {
                    cands.push(e.candidate());
                    origins.push(Origin::Repo {
                        repo: ri,
                        entry: ei,
                    });
                }
            }
        }
        let mut local = Vec::new();
        for f in &sources.files {
            let vp = VerifiedPackage::open(f, &root.keyring, None, dl)?;
            let key = (
                vp.manifest.name.clone(),
                vp.manifest.pkg_version().to_string(),
            );
            if !have.contains(&key) {
                cands.push(candidate_of(&vp.manifest, false));
                origins.push(Origin::Local(local.len()));
            }
            local.push(Some(vp));
        }
        Ok(Universe {
            installed,
            cands,
            origins,
            repos,
            local,
        })
    }
}

fn candidate_of(m: &Manifest, installed: bool) -> Candidate {
    Candidate {
        name: m.name.clone(),
        version: m.pkg_version(),
        depends: m.depends.clone(),
        provides: m.provides.clone(),
        conflicts: m.conflicts.clone(),
        replaces: m.replaces.clone(),
        installed,
    }
}

/// Refuses to overwrite files owned by packages that stay, files that are
/// on disk but owned by nobody, or the same path from two new packages.
fn check_conflicts(
    root: &Root,
    old: &HashMap<String, Manifest>,
    new: &[&Manifest],
    removing: &[String],
) -> Result<()> {
    // Packages whose files may be overwritten: removed ones, and old
    // versions of packages being upgraded.
    let going: Vec<&str> = removing
        .iter()
        .map(String::as_str)
        .chain(new.iter().map(|m| m.name.as_str()))
        .collect();
    let mut claimed: HashMap<&str, &str> = HashMap::new();
    for m in new {
        for f in &m.files {
            let full = root.path.join(&f.path);
            if f.kind == FileKind::Dir {
                // A new directory may not sit where a file or symlink stays.
                if let Ok(md) = fs::symlink_metadata(&full)
                    && !md.is_dir()
                {
                    let owned_by_going = root
                        .db
                        .owners(&f.path)?
                        .iter()
                        .all(|o| going.contains(&o.as_str()));
                    if !owned_by_going || root.db.owners(&f.path)?.is_empty() {
                        return Err(Error::Conflict(format!(
                            "{} wants a directory at {}, which is a file",
                            m.name, f.path
                        )));
                    }
                }
                continue;
            }
            if let Some(other) = claimed.insert(&f.path, &m.name) {
                return Err(Error::Conflict(format!(
                    "{} and {} both contain {}",
                    other, m.name, f.path
                )));
            }
            if let Some(owner) = root.db.file_owner_other_than(&f.path, &going)? {
                return Err(Error::Conflict(format!(
                    "{} contains {}, which belongs to the installed package {owner}",
                    m.name, f.path
                )));
            }
            match fs::symlink_metadata(&full) {
                Ok(md) if md.is_dir() => {
                    return Err(Error::Conflict(format!(
                        "{} contains the file {}, but a directory is there",
                        m.name, f.path
                    )));
                }
                Ok(_) => {
                    let ours = old.values().any(|o| {
                        going.contains(&o.name.as_str()) && o.files.iter().any(|x| x.path == f.path)
                    });
                    if !ours {
                        return Err(Error::Conflict(format!(
                            "{} contains {}, which already exists and is not owned by any package",
                            m.name, f.path
                        )));
                    }
                }
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(e).at(&full),
            }
        }
    }
    Ok(())
}

enum Step {
    CreatedDir(PathBuf),
    Created(PathBuf),
    /// (path in root, backup)
    Replaced(PathBuf, PathBuf),
    Removed(PathBuf, PathBuf),
}

#[cfg(test)]
thread_local! {
    /// Test-only fault injection: fail the Nth journal step (1-based), or the
    /// database commit when set to `usize::MAX`.
    static FAIL_AT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static STEP: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// Returns Ok(()) unconditionally outside tests.
#[cfg_attr(not(test), allow(clippy::unnecessary_wraps))]
fn fault(point: usize) -> Result<()> {
    #[cfg(test)]
    {
        let n = STEP.with(|s| {
            s.set(s.get() + 1);
            s.get()
        });
        let hit = FAIL_AT.with(std::cell::Cell::get).is_some_and(|f| {
            (f == usize::MAX && point == usize::MAX)
                || (f != usize::MAX && point != usize::MAX && f == n)
        });
        if hit {
            return Err(Error::Conflict(format!("injected failure at step {n}")));
        }
    }
    let _ = point;
    Ok(())
}

/// Undo log for one transaction's file changes.
struct Journal {
    dir: PathBuf,
    steps: Vec<Step>,
    n: usize,
}

impl Journal {
    fn new(staging: &Path) -> Result<Self> {
        let dir = staging.join("backup");
        fs::create_dir(&dir).at(&dir)?;
        let mark = staging.join(JOURNAL_MARK);
        fs::write(&mark, b"").at(&mark)?;
        Ok(Journal {
            dir,
            steps: Vec::new(),
            n: 0,
        })
    }

    fn backup_path(&mut self) -> PathBuf {
        self.n += 1;
        self.dir.join(self.n.to_string())
    }

    /// Puts `src` (a staged file or symlink) at `dst`, keeping any previous
    /// entry at `dst` as a backup.
    fn place(&mut self, src: &Path, dst: &Path) -> Result<()> {
        fault(0)?;
        match fs::symlink_metadata(dst) {
            Ok(_) => {
                let b = self.backup_path();
                fs::hard_link(dst, &b).at(dst)?;
                fs::rename(src, dst).at(dst)?;
                self.steps.push(Step::Replaced(dst.to_path_buf(), b));
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                fs::rename(src, dst).at(dst)?;
                self.steps.push(Step::Created(dst.to_path_buf()));
            }
            Err(e) => return Err(e).at(dst),
        }
        Ok(())
    }

    fn mkdir(&mut self, dst: &Path, mode: u32) -> Result<()> {
        fault(0)?;
        match fs::symlink_metadata(dst) {
            Ok(md) if md.is_dir() => Ok(()),
            Ok(_) => Err(Error::Conflict(format!(
                "{} is not a directory",
                dst.display()
            ))),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                fs::create_dir(dst).at(dst)?;
                fs::set_permissions(dst, fs::Permissions::from_mode(mode)).at(dst)?;
                self.steps.push(Step::CreatedDir(dst.to_path_buf()));
                Ok(())
            }
            Err(e) => Err(e).at(dst),
        }
    }

    fn remove(&mut self, dst: &Path) -> Result<()> {
        fault(0)?;
        match fs::symlink_metadata(dst) {
            Ok(md) if md.is_dir() => Ok(()),
            Ok(_) => {
                let b = self.backup_path();
                fs::rename(dst, &b).at(dst)?;
                self.steps.push(Step::Removed(dst.to_path_buf(), b));
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).at(dst),
        }
    }

    fn rollback(&mut self) -> Result<()> {
        let mut first_err = None;
        while let Some(step) = self.steps.pop() {
            let r = match &step {
                Step::Created(p) => fs::remove_file(p).at(p),
                Step::CreatedDir(p) => fs::remove_dir(p).at(p),
                Step::Replaced(p, b) | Step::Removed(p, b) => fs::rename(b, p).at(p),
            };
            if let Err(e) = r {
                first_err.get_or_insert(e);
            }
        }
        match first_err {
            None => {
                let _ = fs::remove_file(self.dir.with_file_name(JOURNAL_MARK));
                Ok(())
            }
            Some(e) => Err(e),
        }
    }

    fn finish(self) {
        let _ = fs::remove_file(self.dir.with_file_name(JOURNAL_MARK));
    }
}

fn apply(
    root: &Root,
    old: &HashMap<String, Manifest>,
    new: &[&Manifest],
    extracted: &[PathBuf],
    removing: &[String],
    j: &mut Journal,
) -> Result<()> {
    let mut new_paths: HashSet<&str> = HashSet::new();
    for (m, src) in new.iter().zip(extracted) {
        for f in &m.files {
            let dst = root.path.join(&f.path);
            match f.kind {
                FileKind::Dir => j.mkdir(&dst, f.mode.unwrap_or(0o755))?,
                FileKind::File | FileKind::Symlink => j.place(&src.join(&f.path), &dst)?,
            }
            new_paths.insert(&f.path);
        }
    }
    // Files of removed packages and of old versions that the new set no
    // longer contains.
    let leaving = removing
        .iter()
        .chain(new.iter().map(|m| &m.name))
        .filter_map(|n| old.get(n));
    for m in leaving {
        for p in non_dir_paths(m) {
            if !new_paths.contains(p) {
                j.remove(&root.path.join(p))?;
            }
        }
    }
    Ok(())
}

fn commit_db(
    root: &mut Root,
    req: &Request,
    old: &HashMap<String, Manifest>,
    new: &[&Manifest],
    removing: &[String],
) -> Result<()> {
    let requested: HashSet<&str> = req
        .install
        .iter()
        .map(|d: &DepSpec| d.name.as_str())
        .collect();
    let explicit_before: HashMap<String, bool> = root
        .db
        .installed()?
        .into_iter()
        .map(|i| (i.manifest.name, i.explicit))
        .collect();
    let ts = db::now();
    let tx = root.db.begin()?;
    for name in removing {
        tx.remove_package(name)?;
        let ov = old.get(name).map(|m| m.pkg_version().to_string());
        tx.record(ts, "remove", name, ov.as_deref(), None)?;
    }
    for m in new {
        let explicit = requested.contains(m.name.as_str())
            || explicit_before.get(&m.name).copied().unwrap_or(false)
            || m.provides
                .iter()
                .any(|p| requested.contains(p.name.as_str()));
        tx.put_package(m, explicit, ts)?;
        let ov = old.get(&m.name).map(|o| o.pkg_version().to_string());
        let action = if ov.is_some() { "upgrade" } else { "install" };
        tx.record(
            ts,
            action,
            &m.name,
            ov.as_deref(),
            Some(&m.pkg_version().to_string()),
        )?;
    }
    // Explicitly re-requesting an installed dependency marks it explicit.
    for name in &requested {
        if !new.iter().any(|m| m.name == *name) && explicit_before.get(*name) == Some(&false) {
            tx.set_explicit(name, true)?;
        }
    }
    fault(usize::MAX)?;
    tx.commit()
}

/// Removes now-empty directories that no remaining package owns. Best
/// effort: a directory that is not empty simply stays.
fn prune_dirs(
    root: &Root,
    old: &HashMap<String, Manifest>,
    new: &[&Manifest],
    removing: &[String],
) {
    let mut dirs: Vec<&str> = removing
        .iter()
        .chain(new.iter().map(|m| &m.name))
        .filter_map(|n| old.get(n))
        .flat_map(|m| {
            m.files
                .iter()
                .filter(|f| f.kind == FileKind::Dir)
                .map(|f| f.path.as_str())
        })
        .collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.len()));
    dirs.dedup();
    for d in dirs {
        if root.db.owners(d).is_ok_and(|o| o.is_empty()) {
            let _ = fs::remove_dir(root.path.join(d));
        }
    }
}

/// True when `path` inside the root is a regular file whose content no
/// longer matches the package database (for `spk verify` later).
pub fn is_modified(root: &Path, entry: &crate::manifest::FileEntry) -> bool {
    let p = root.join(&entry.path);
    match entry.kind {
        FileKind::File => fs::symlink_metadata(&p).map_or(true, |md| {
            !md.is_file()
                || Some(md.size()) != entry.size
                || crate::hash::sha256_file(&p)
                    .map_or(true, |(d, _)| Some(hex::encode(d)) != entry.sha256)
        }),
        FileKind::Symlink => {
            fs::read_link(&p).map_or(true, |t| t.to_str() != entry.target.as_deref())
        }
        FileKind::Dir => !p.is_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::tests::{Fixture, build_pkg, meta};
    use crate::repo::repo_add;
    use crate::sign::ADMIN_KEY_DIR;

    struct Env {
        fx: Fixture,
        root: PathBuf,
        repo: PathBuf,
    }

    fn env(pkgs: &[(&str, &str, &[&str])]) -> Env {
        let fx = Fixture::new();
        let files: Vec<PathBuf> = pkgs
            .iter()
            .map(|(n, v, d)| build_pkg(&fx, &meta(n, v, d), &format!("{n}.txt")))
            .collect();
        let repo = fx.path("repo");
        repo_add(&repo, "t", &files, &fx.key, &fx.ring).unwrap();
        let root = fx.path("root");
        fs::create_dir_all(root.join(ADMIN_KEY_DIR)).unwrap();
        fs::write(
            root.join(ADMIN_KEY_DIR).join("dev.pub"),
            fx.key.public().to_text(),
        )
        .unwrap();
        Env { fx, root, repo }
    }

    fn req_install(n: &[&str]) -> Request {
        Request {
            install: n.iter().map(|d| DepSpec::parse(d).unwrap()).collect(),
            ..Request::default()
        }
    }

    fn go(e: &Env, req: &Request) -> Result<Plan> {
        let mut root = Root::open(&e.root)?;
        let src = Sources {
            repos: vec![Location::Local(e.repo.clone())],
            files: vec![],
        };
        run(&mut root, req, &src, &mut |_| {})
    }

    #[test]
    fn install_with_deps_then_remove() {
        let e = env(&[("app", "1.0", &["lib"]), ("lib", "2.0", &[])]);
        go(&e, &req_install(&["app"])).unwrap();
        assert!(e.root.join("usr/bin/app").is_file());
        assert!(e.root.join("usr/bin/lib").is_file());
        let root = Root::open(&e.root).unwrap();
        assert!(root.db.get("app").unwrap().unwrap().explicit);
        assert!(!root.db.get("lib").unwrap().unwrap().explicit);
        assert_eq!(root.db.owners("usr/share/doc/lib.txt").unwrap(), ["lib"]);
        drop(root);

        // lib is needed by app.
        let rm = |names: &[&str]| Request {
            remove: names.iter().map(ToString::to_string).collect(),
            ..Request::default()
        };
        assert!(go(&e, &rm(&["lib"])).is_err());
        assert!(
            e.root.join("usr/bin/lib").is_file(),
            "failed remove changes nothing"
        );

        go(&e, &rm(&["app", "lib"])).unwrap();
        assert!(!e.root.join("usr/bin/app").exists());
        assert!(!e.root.join("usr/share/doc").exists(), "empty dirs pruned");
        assert!(!e.root.join(STAGING).exists());
        let root = Root::open(&e.root).unwrap();
        assert!(root.db.installed().unwrap().is_empty());
        assert_eq!(root.db.history().unwrap().len(), 4);
    }

    #[test]
    fn refuses_without_trusted_keys() {
        let e = env(&[("app", "1.0", &[])]);
        fs::remove_dir_all(e.root.join(ADMIN_KEY_DIR)).unwrap();
        let err = go(&e, &req_install(&["app"])).unwrap_err().to_string();
        assert!(err.contains("trusts no signing keys"), "{err}");
    }

    #[test]
    fn refuses_untracked_file_and_leaves_root_alone() {
        let e = env(&[("app", "1.0", &[])]);
        fs::create_dir_all(e.root.join("usr/bin")).unwrap();
        fs::write(e.root.join("usr/bin/app"), b"mine").unwrap();
        let err = go(&e, &req_install(&["app"])).unwrap_err().to_string();
        assert!(err.contains("not owned by any package"), "{err}");
        assert_eq!(fs::read(e.root.join("usr/bin/app")).unwrap(), b"mine");
        assert!(!e.root.join("usr/share/doc/app.txt").exists());
    }

    #[test]
    fn file_conflict_between_packages() {
        let e = env(&[("a", "1.0", &[])]);
        go(&e, &req_install(&["a"])).unwrap();
        // A second package shipping usr/bin/a.
        let fx = &e.fx;
        let src = fx.path("srcb");
        fs::create_dir_all(src.join("usr/bin")).unwrap();
        fs::write(src.join("usr/bin/a"), b"other").unwrap();
        let (p, _) = crate::package::create(
            &src,
            &["usr".into(), "usr/bin".into(), "usr/bin/a".into()],
            &meta("b", "1.0", &[]),
            &fx.path("out"),
            &fx.key,
        )
        .unwrap();
        let mut root = Root::open(&e.root).unwrap();
        let err = run(
            &mut root,
            &req_install(&["b"]),
            &Sources {
                repos: vec![],
                files: vec![p],
            },
            &mut |_| {},
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("belongs to the installed package a"), "{err}");
    }

    #[test]
    fn upgrade_replaces_files_and_drops_old_ones() {
        let e = env(&[("app", "1.0", &[])]);
        go(&e, &req_install(&["app"])).unwrap();
        // 1.1 renames its doc file.
        let p = build_pkg(&e.fx, &meta("app", "1.1", &[]), "app-new.txt");
        repo_add(&e.repo, "t", &[p], &e.fx.key, &e.fx.ring).unwrap();
        go(
            &e,
            &Request {
                upgrade: true,
                ..Request::default()
            },
        )
        .unwrap();
        assert!(e.root.join("usr/share/doc/app-new.txt").exists());
        assert!(!e.root.join("usr/share/doc/app.txt").exists());
        let root = Root::open(&e.root).unwrap();
        let m = root.db.get("app").unwrap().unwrap().manifest;
        assert_eq!(m.version.as_str(), "1.1");
        assert!(
            root.db.get("app").unwrap().unwrap().explicit,
            "explicit survives upgrade"
        );
        let h = root.db.history().unwrap();
        assert_eq!(h.last().unwrap().action, "upgrade");
    }

    /// Every file and symlink under `root`, with contents/targets.
    fn snapshot(root: &Path) -> Vec<(String, String)> {
        let mut v = Vec::new();
        for e in walkdir::WalkDir::new(root).sort_by_file_name() {
            let e = e.unwrap();
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if rel.starts_with("usr/lib/spk") {
                continue;
            }
            let what = if e.path_is_symlink() {
                format!("-> {}", fs::read_link(e.path()).unwrap().display())
            } else if e.file_type().is_file() {
                crate::hash::sha256_hex(&fs::read(e.path()).unwrap())
            } else {
                "dir".into()
            };
            v.push((rel, what));
        }
        v
    }

    #[test]
    fn injected_failures_roll_back_completely() {
        // Upgrade app 1.0 -> 1.1 (which changes, adds, and drops files) and
        // install a new dependency, failing at every single step in turn.
        let e = env(&[("app", "1.0", &[])]);
        go(&e, &req_install(&["app"])).unwrap();
        let app11 = build_pkg(&e.fx, &meta("app", "1.1", &["dep"]), "app-new.txt");
        let dep = build_pkg(&e.fx, &meta("dep", "1.0", &[]), "dep.txt");
        repo_add(&e.repo, "t", &[app11, dep], &e.fx.key, &e.fx.ring).unwrap();
        let before = snapshot(&e.root);
        let upgrade = Request {
            upgrade: true,
            ..Request::default()
        };

        let check_restored = |at: usize| {
            assert_eq!(
                snapshot(&e.root),
                before,
                "not restored after failing at {at}"
            );
            assert!(!e.root.join(STAGING).exists());
            let root = Root::open(&e.root).unwrap();
            let app = root.db.get("app").unwrap().unwrap();
            assert_eq!(app.manifest.version.as_str(), "1.0");
            assert!(root.db.get("dep").unwrap().is_none());
        };
        let attempt = |at: usize| {
            FAIL_AT.with(|f| f.set(Some(at)));
            STEP.with(|s| s.set(0));
            let r = go(&e, &upgrade);
            FAIL_AT.with(|f| f.set(None));
            r
        };

        // The database commit fails after every file is in place.
        let err = attempt(usize::MAX).unwrap_err();
        assert!(err.to_string().contains("injected"), "{err}");
        check_restored(usize::MAX);

        // Then each file step in turn, until the transaction has fewer
        // steps than the injection point and simply succeeds.
        let mut at = 1;
        while let Err(err) = attempt(at) {
            assert!(err.to_string().contains("injected"), "{err}");
            check_restored(at);
            at += 1;
        }
        assert!(at > 10, "too few steps exercised: {at}");

        assert!(e.root.join("usr/share/doc/app-new.txt").exists());
        assert!(!e.root.join("usr/share/doc/app.txt").exists());
        let root = Root::open(&e.root).unwrap();
        let m = root.db.get("app").unwrap().unwrap().manifest;
        assert_eq!(m.version.as_str(), "1.1");
        assert!(
            root.db.get("app").unwrap().unwrap().explicit,
            "explicit survives upgrade"
        );
        let h = root.db.history().unwrap();
        assert!(
            h.iter()
                .any(|x| x.action == "upgrade" && x.package == "app")
        );
        assert!(
            h.iter()
                .any(|x| x.action == "install" && x.package == "dep")
        );
        assert!(e.root.join("usr/bin/dep").exists());
    }

    #[test]
    fn interrupted_transaction_blocks_next_run() {
        let e = env(&[("app", "1.0", &[])]);
        fs::create_dir_all(e.root.join(STAGING)).unwrap();
        let err = go(&e, &req_install(&["app"])).unwrap_err().to_string();
        assert!(err.contains("interrupted"), "{err}");
    }

    #[test]
    fn lock_is_exclusive() {
        let e = env(&[("app", "1.0", &[])]);
        let _held = Root::open(&e.root).unwrap();
        assert!(Root::open(&e.root).is_err());
    }

    #[test]
    fn modified_detection() {
        let e = env(&[("app", "1.0", &[])]);
        go(&e, &req_install(&["app"])).unwrap();
        let root = Root::open(&e.root).unwrap();
        let m = root.db.get("app").unwrap().unwrap().manifest;
        let f = m.files.iter().find(|f| f.path == "usr/bin/app").unwrap();
        assert!(!is_modified(&e.root, f));
        fs::write(e.root.join("usr/bin/app"), b"tampered").unwrap();
        assert!(is_modified(&e.root, f));
    }
}
