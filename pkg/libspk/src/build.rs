//! Building recipes into signed packages inside a bubblewrap sandbox.
//!
//! Security-critical (CLAUDE.md rule 7: never run build scripts as root).
//! Spec: docs/formats/recipe.md, "Build environment" and "Output
//! normalisation".
//!
//! Trust boundary: everything the build script produces is hostile until
//! checked. After the sandbox exits (its PID namespace takes every build
//! process with it), spk inspects `DESTDIR` without following symlinks and
//! refuses anything outside `usr/`.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, IoContext, Result};
use crate::fetch::{self, Location};
use crate::hash;
use crate::manifest::validate_payload_path;
use crate::package::{self, PackageMeta};
use crate::recipe::{Recipe, Source};
use crate::repo::{self, Repo};
use crate::sign::{ADMIN_KEY_DIR, Keyring, PublicKey, SecretKey};
use crate::solve::Request;
use crate::tx::{self, Root, Sources};
use crate::version::{Constraint, DepSpec, Op};

/// `SOURCE_DATE_EPOCH` for every build: 1980-01-01, the earliest date ZIP
/// can store, so archives and timestamps are reproducible.
pub const SOURCE_DATE_EPOCH: u64 = 315_532_800;
/// Unprivileged account builds drop to when spk runs as root.
pub const DEFAULT_BUILD_UID: u32 = 65534;
/// The uid the build sees inside its user namespace.
const SANDBOX_UID: u32 = 1000;
const LOG_TAIL_LINES: usize = 40;

pub use crate::recipe::ToolchainMode;

/// Default target triple. Differs from the host's so that bootstrap stage 0
/// and 1 are true cross builds (nothing from the host leaks in).
pub const TARGET: &str = "x86_64-solace-linux-gnu";

/// The host's tools (the Nix devShell), used by `host` and `cross` builds.
#[derive(Clone, Debug)]
pub struct HostTools {
    /// The devShell's `/nix/store/...` PATH entries.
    pub path: String,
    pub bash: PathBuf,
    pub env: PathBuf,
}

impl HostTools {
    /// Detects the Nix devShell's tools from `$PATH`.
    ///
    /// # Errors
    /// [`Error::Build`] outside a Nix devShell.
    pub fn detect() -> Result<Self> {
        let path = std::env::var("PATH").unwrap_or_default();
        let store_path: Vec<&str> = path
            .split(':')
            .filter(|p| p.starts_with("/nix/store/"))
            .collect();
        let find = |exe: &str| {
            store_path
                .iter()
                .map(|d| Path::new(d).join(exe))
                .find(|p| p.is_file())
        };
        let (Some(bash), Some(env), Some(_)) = (find("bash"), find("env"), find("gcc")) else {
            return Err(Error::Build(
                "no Nix toolchain on PATH: host and cross builds run inside `nix develop`".into(),
            ));
        };
        Ok(HostTools {
            path: store_path.join(":"),
            bash,
            env,
        })
    }

    /// A stable identifier: changing the host tools invalidates stamps.
    pub fn id(&self) -> String {
        hash::sha256_hex(self.path.as_bytes())
    }
}

#[derive(Debug)]
pub struct BuildConfig {
    /// Per-recipe work dirs live here (`<work>/<recipe>/`).
    pub work: PathBuf,
    /// Content-addressed source cache.
    pub sources: PathBuf,
    /// Local sources (`[[source]] local = ...`) are relative to this.
    pub repo_root: PathBuf,
    /// Local repository: build deps come from it, results go into it.
    pub repo: PathBuf,
    pub repo_name: String,
    pub key: SecretKey,
    /// Keys trusted when installing build deps (the signing key's public
    /// key is always included).
    pub trusted: Vec<PublicKey>,
    /// The host's tools; required for `host` and `cross` builds only.
    pub host: Option<HostTools>,
    /// Mode for recipes that do not set `build.toolchain`.
    pub default_mode: ToolchainMode,
    /// Forces a mode for every recipe (e.g. the Phase 1 demo's `host`).
    pub mode_override: Option<ToolchainMode>,
    /// Further repositories build deps may come from (e.g. the bootstrap
    /// repo). Explicit build deps take the newest version across all
    /// repos; the base set is pinned to these repos ([`pin_base_deps`]).
    pub dep_repos: Vec<PathBuf>,
    /// Installed into every sysroot of the stage: the compiler, make,
    /// shell...
    pub base_deps: Vec<DepSpec>,
    /// How the stage's own packages relate (splits, runtime deps); see
    /// [`merge_base`].
    pub stage: StageGraph,
    /// `$SOL_TGT` in build scripts.
    pub target: String,
    /// Keep `build/` and `sysroot/` after a successful build (they can be
    /// several GB); the log is always kept.
    pub keep_work: bool,
    pub jobs: usize,
    pub build_uid: u32,
    /// Rebuild even if the stamp says nothing changed.
    pub force: bool,
}

#[derive(Clone, Debug)]
pub enum BuildEvent {
    Skipped { recipe: String },
    Fetching { url: String },
    InstallingDeps { count: usize },
    Phase { recipe: String, phase: String },
    Packaged { path: PathBuf },
}

/// Result of building one recipe.
#[derive(Debug)]
pub struct Built {
    pub packages: Vec<PathBuf>,
    pub skipped: bool,
}

fn effective_uid() -> u32 {
    // The owner of /proc/self is the process's effective uid.
    fs::metadata("/proc/self").map_or(u32::MAX, |m| m.uid())
}

/// The toolchain mode `recipe` builds with under `cfg`.
pub fn mode_of(recipe: &Recipe, cfg: &BuildConfig) -> ToolchainMode {
    cfg.mode_override
        .or(recipe.build.toolchain)
        .unwrap_or(cfg.default_mode)
}

fn host_of(cfg: &BuildConfig, mode: ToolchainMode) -> Result<Option<&HostTools>> {
    match mode {
        ToolchainMode::Native => Ok(None),
        ToolchainMode::Host | ToolchainMode::Cross | ToolchainMode::Rust => {
            cfg.host.as_ref().map(Some).ok_or_else(|| {
                Error::Build(format!(
                    "{mode:?} builds need the host tools; run inside `nix develop`"
                ))
            })
        }
    }
}

/// Build-dep repositories that exist (the output repo first).
fn dep_locations(cfg: &BuildConfig) -> Vec<Location> {
    std::iter::once(&cfg.repo)
        .chain(&cfg.dep_repos)
        .filter(|r| r.join(repo::INDEX).exists())
        .map(|r| Location::Local(r.clone()))
        .collect()
}

/// The packages a stage's recipes produce: which split comes from which
/// recipe, and each package's runtime deps (by name).
#[derive(Clone, Debug, Default)]
pub struct StageGraph {
    pub split_parents: HashMap<String, String>,
    pub runtime: HashMap<String, Vec<String>>,
}

impl StageGraph {
    pub fn from_recipes(recipes: &[Recipe]) -> Self {
        let mut g = StageGraph::default();
        for r in recipes {
            let names = |d: &[DepSpec]| d.iter().map(|d| d.name.clone()).collect::<Vec<_>>();
            g.runtime.insert(r.name.clone(), names(&r.deps.runtime));
            for s in &r.splits {
                g.split_parents.insert(s.name.clone(), r.name.clone());
                g.runtime.insert(s.name.clone(), names(&s.depends));
            }
        }
        g
    }

    /// `roots` and everything they need at runtime, as far as the stage's
    /// own packages are known.
    fn closure<'a>(&'a self, roots: impl Iterator<Item = &'a str>) -> BTreeSet<&'a str> {
        let mut seen = BTreeSet::new();
        let mut todo: Vec<&str> = roots.collect();
        while let Some(n) = todo.pop() {
            if seen.insert(n) {
                todo.extend(
                    self.runtime
                        .get(n)
                        .into_iter()
                        .flatten()
                        .map(String::as_str),
                );
            }
        }
        seen
    }
}

/// Everything installed into the sysroot for `recipe`.
fn sysroot_deps(recipe: &Recipe, cfg: &BuildConfig) -> Vec<DepSpec> {
    if mode_of(recipe, cfg) == ToolchainMode::Host {
        // Host builds use the host's tools; the stage's base set is for the
        // stage's own toolchain.
        return recipe.deps.build.clone();
    }
    merge_base(&recipe.deps.build, &cfg.base_deps, &cfg.stage)
}

/// A recipe's build deps plus the stage's base set. A base package the
/// recipe names itself is dropped. When a build dep, or anything it needs
/// at runtime, is a split (`liblzma` from `xz`), the base copy of its parent
/// is swapped for the stage's own parent package: an unsplit base `xz`
/// would carry the same files.
fn merge_base(build: &[DepSpec], base: &[DepSpec], stage: &StageGraph) -> Vec<DepSpec> {
    let needed = stage.closure(build.iter().map(|d| d.name.as_str()));
    let parents: Vec<&String> = needed
        .iter()
        .filter_map(|n| stage.split_parents.get(*n))
        .collect();
    let mut deps = build.to_vec();
    for b in base {
        if deps.iter().any(|d| d.name == b.name) {
            continue;
        }
        if parents.contains(&&b.name) {
            deps.push(DepSpec {
                name: b.name.clone(),
                constraint: None,
            });
        } else {
            deps.push(b.clone());
        }
    }
    deps
}

/// Pins each unversioned base dependency to the newest version in the
/// stage's dependency repositories (never the stage's own output repo).
/// Without this, a package's sysroot would change as the stage builds
/// newer versions of its base packages, and every rerun would rebuild
/// everything built before them. A stage without dependency repos takes
/// its base from its own repo and is left as is.
///
/// # Errors
/// Repository errors, or a base package that no dependency repo has.
pub fn pin_base_deps(cfg: &mut BuildConfig) -> Result<()> {
    if cfg.dep_repos.is_empty() || cfg.base_deps.iter().all(|d| d.constraint.is_some()) {
        return Ok(());
    }
    let keyring = keyring_of(cfg);
    let tmp = tempfile::tempdir().at(std::env::temp_dir())?;
    let mut entries = Vec::new();
    for r in cfg
        .dep_repos
        .iter()
        .filter(|r| r.join(repo::INDEX).exists())
    {
        entries.extend(
            Repo::open(Location::Local(r.clone()), &keyring, tmp.path())?
                .index
                .packages,
        );
    }
    pin_to_newest(&mut cfg.base_deps, &entries)
}

fn pin_to_newest(base: &mut [DepSpec], entries: &[repo::IndexEntry]) -> Result<()> {
    for dep in base.iter_mut().filter(|d| d.constraint.is_none()) {
        let newest = entries
            .iter()
            .filter(|e| e.name == dep.name)
            .map(repo::IndexEntry::pkg_version)
            .max()
            .ok_or_else(|| {
                Error::invalid(format!(
                    "base package {} is in no dependency repository",
                    dep.name
                ))
            })?;
        dep.constraint = Some(Constraint {
            op: Op::Eq,
            version: newest.version,
            release: Some(newest.release),
        });
    }
    Ok(())
}

/// Builds every package of `recipe`. See the module docs for the steps.
///
/// # Errors
/// Fetch, sandbox, build-script, and output-validation errors. The build
/// log's tail is included in [`Error::Build`].
pub fn build(recipe: &Recipe, cfg: &BuildConfig, on: &mut dyn FnMut(BuildEvent)) -> Result<Built> {
    let keyring = keyring_of(cfg);
    let mode = mode_of(recipe, cfg);
    let host = host_of(cfg, mode)?;
    let work = cfg.work.join(&recipe.name);
    let stamp_path = cfg
        .work
        .join("stamps")
        .join(format!("{}.stamp", recipe.name));

    // 0. Skip if nothing changed.
    let stamp = build_stamp(recipe, cfg, &keyring)?;
    if !cfg.force
        && fs::read_to_string(&stamp_path).is_ok_and(|s| s == stamp)
        && outputs_in_repo(recipe, cfg, &keyring)?
    {
        on(BuildEvent::Skipped {
            recipe: recipe.name.clone(),
        });
        return Ok(Built {
            packages: Vec::new(),
            skipped: true,
        });
    }

    // 1. Sources (verified against the recipe's sha256).
    let mut fetched = Vec::new();
    for s in recipe.sources.iter().filter(|s| !s.is_local()) {
        on(BuildEvent::Fetching { url: s.url.clone() });
        fetched.push((s.clone(), fetch::fetch_source(s, &cfg.sources)?));
    }

    // 2. Fresh work dir.
    let (sysroot, bdir, log_path) = prepare_workdir(&work, recipe, &fetched)?;
    for s in recipe.sources.iter().filter(|s| s.is_local()) {
        let src = local_path(&cfg.repo_root, s)?;
        copy_tree(&src, &bdir.join("src").join(s.file_name()))?;
    }
    let vendor = if recipe.build.cargo {
        on(BuildEvent::Fetching {
            url: "crates (cargo vendor)".into(),
        });
        let first = &recipe.sources[0];
        let dir = cargo_vendor(cfg, host, &local_path(&cfg.repo_root, first)?)?;
        write_cargo_config(&bdir)?;
        Some(dir)
    } else {
        None
    };

    // 3. Build deps into the sysroot, through the verified install path.
    prepare_sysroot(recipe, cfg, host, &sysroot, on)?;

    // 4. Hand the build dir to the build user and run the sandbox.
    let run_uid = if effective_uid() == 0 {
        Some(cfg.build_uid)
    } else {
        None
    };
    if let Some(uid) = run_uid {
        chown_tree(&bdir, uid)?;
    }
    let mut log = File::create(&log_path).at(&log_path)?;
    on(BuildEvent::Phase {
        recipe: recipe.name.clone(),
        phase: "extract".into(),
    });
    let extract = extract_script(&fetched);
    let sb = Sandbox {
        cfg,
        mode,
        host,
        sysroot: &sysroot,
        bdir: &bdir,
        etc: &work.join("etc"),
        vendor: vendor.as_deref(),
    };
    run_sandbox(&sb, "/build/src", &extract, run_uid, &mut log, &log_path)?;
    let srcdir = source_dir(&bdir.join("src"), &recipe.sources)?;
    on(BuildEvent::Phase {
        recipe: recipe.name.clone(),
        phase: "build".into(),
    });
    let chdir = format!("/build/src/{srcdir}");
    run_sandbox(
        &sb,
        &chdir,
        &recipe.build_script(),
        run_uid,
        &mut log,
        &log_path,
    )?;

    // 5. Check and package the output.
    let dest = bdir.join("dest");
    normalise_dest(&dest)?;
    let all = list_dest(&dest)?;
    let groups = split_paths(recipe, &all)?;
    let recipe_sha = recipe.content_hash()?;
    let mut out = Vec::new();
    for (pkg_name, paths) in groups {
        let meta = package_meta(recipe, &pkg_name, &recipe_sha);
        let (p, _) = package::create(&dest, &paths, &meta, &cfg.repo.join("incoming"), &cfg.key)?;
        on(BuildEvent::Packaged { path: p.clone() });
        out.push(p);
    }

    // 6. Publish into the local repo, record the stamp, clean up.
    let published = publish(cfg, &keyring, &out, &stamp_path, &stamp, &[&bdir, &sysroot])?;
    Ok(Built {
        packages: published,
        skipped: false,
    })
}

/// Adds freshly built packages to the repo, records the build stamp, and
/// (unless `keep_work`) removes the work trees.
fn publish(
    cfg: &BuildConfig,
    keyring: &Keyring,
    out: &[PathBuf],
    stamp_path: &Path,
    stamp: &str,
    work_trees: &[&Path],
) -> Result<Vec<PathBuf>> {
    repo::repo_add(&cfg.repo, &cfg.repo_name, out, &cfg.key, keyring)?;
    for p in out {
        let _ = fs::remove_file(p);
        let _ = fs::remove_file(crate::sign::sig_path(p));
    }
    if let Some(d) = stamp_path.parent() {
        fs::create_dir_all(d).at(d)?;
    }
    fs::write(stamp_path, stamp).at(stamp_path)?;
    if !cfg.keep_work {
        for d in work_trees {
            make_removable(d);
            let _ = fs::remove_dir_all(d);
        }
    }
    let published = out
        .iter()
        .filter_map(|p| p.file_name())
        .map(|f| cfg.repo.join("packages").join(f))
        .collect();
    Ok(published)
}

/// Resolves a local source, refusing anything outside the repository.
fn local_path(repo_root: &Path, s: &Source) -> Result<PathBuf> {
    let rel = s.local.as_deref().unwrap_or_default();
    let root = repo_root.canonicalize().at(repo_root)?;
    let joined = root.join(rel);
    let resolved = joined.canonicalize().at(&joined)?;
    if !resolved.starts_with(&root) || !resolved.is_dir() {
        return Err(Error::Build(format!(
            "local source {rel} must be a directory inside the repository"
        )));
    }
    Ok(resolved)
}

/// Vendors the crates in `<src>/Cargo.lock` into a shared cache keyed by
/// the lockfile's hash (cargo verifies every crate's checksum against the
/// lockfile while vendoring). Runs on the host, before the sandbox.
fn cargo_vendor(cfg: &BuildConfig, host: Option<&HostTools>, src: &Path) -> Result<PathBuf> {
    let lock = src.join("Cargo.lock");
    let key = hash::sha256_hex(&fs::read(&lock).at(&lock)?);
    let dir = cfg.sources.join("cargo-vendor").join(&key[..16]);
    if dir.join(".complete").exists() {
        return Ok(dir);
    }
    let host_path = std::env::var("PATH").unwrap_or_default();
    let cargo = host
        .and_then(|h| which(&h.path, "cargo"))
        .or_else(|| which(&host_path, "cargo"))
        .ok_or_else(|| Error::Build("cargo not found on the host".into()))?;
    let tmp = cfg
        .sources
        .join("cargo-vendor")
        .join(format!("{}.part", &key[..16]));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).at(&tmp)?;
    let out = Command::new(cargo)
        .args(["vendor", "--locked", "--versioned-dirs", "--manifest-path"])
        .arg(src.join("Cargo.toml"))
        .arg(&tmp)
        .stdout(Stdio::null())
        .output()
        .map_err(|e| Error::Build(format!("cargo vendor: {e}")))?;
    if !out.status.success() {
        return Err(Error::Build(format!(
            "cargo vendor failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    fs::write(tmp.join(".complete"), "").at(&tmp)?;
    fs::rename(&tmp, &dir).at(&dir)?;
    Ok(dir)
}

/// Points cargo at /build/vendor instead of crates.io.
fn write_cargo_config(bdir: &Path) -> Result<()> {
    let home = bdir.join("cargo-home");
    fs::create_dir_all(&home).at(&home)?;
    fs::create_dir_all(bdir.join("vendor")).at(bdir.join("vendor"))?;
    let cfg = "[source.crates-io]\nreplace-with = \"vendored\"\n\n\
               [source.vendored]\ndirectory = \"/build/vendor\"\n";
    fs::write(home.join("config.toml"), cfg).at(home.join("config.toml"))
}

/// Directories never copied from a local source.
const LOCAL_SKIP: &[&str] = &[".git", "target", "build", "result", ".direnv"];

/// Copies a local source tree: regular files and directories only (no
/// symlinks followed or copied), skipping build outputs.
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    let walk = walkdir::WalkDir::new(src)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !LOCAL_SKIP.iter().any(|s| e.file_name() == *s));
    for e in walk {
        let e = e.map_err(|e| Error::Build(e.to_string()))?;
        let rel = e.path().strip_prefix(src).unwrap_or(e.path());
        let to = dst.join(rel);
        if e.file_type().is_dir() {
            fs::create_dir_all(&to).at(&to)?;
        } else if e.file_type().is_file() {
            fs::copy(e.path(), &to).at(&to)?;
        }
    }
    Ok(())
}

/// sha256 over a local source tree (paths and contents), for stamps.
fn tree_hash(dir: &Path) -> Result<String> {
    let mut text = String::new();
    let walk = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !LOCAL_SKIP.iter().any(|s| e.file_name() == *s));
    for e in walk {
        let e = e.map_err(|e| Error::Build(e.to_string()))?;
        if e.file_type().is_file() {
            let rel = e
                .path()
                .strip_prefix(dir)
                .unwrap_or(e.path())
                .to_string_lossy();
            let data = fs::read(e.path()).at(e.path())?;
            let _ = writeln!(text, "{} {rel}", hash::sha256_hex(&data));
        }
    }
    Ok(hash::sha256_hex(text.as_bytes()))
}

/// Wipes and recreates `<work>/`: `sysroot/`, `build/{src,dest,home,
/// sources,recipe}`; copies the verified sources and the recipe dir in.
fn prepare_workdir(
    work: &Path,
    recipe: &Recipe,
    fetched: &[(Source, PathBuf)],
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    if work.exists() {
        make_removable(work);
        fs::remove_dir_all(work).at(work)?;
    }
    let sysroot = work.join("sysroot");
    let bdir = work.join("build");
    for d in ["src", "dest", "home", "sources", "recipe"] {
        fs::create_dir_all(bdir.join(d)).at(bdir.join(d))?;
    }
    for (s, cached) in fetched {
        let dst = bdir.join("sources").join(s.file_name());
        fs::copy(cached, &dst).at(&dst)?;
    }
    copy_dir(&recipe.dir, &bdir.join("recipe"))?;
    Ok((sysroot, bdir, work.join("build.log")))
}

fn keyring_of(cfg: &BuildConfig) -> Keyring {
    let mut keys = cfg.trusted.clone();
    keys.push(cfg.key.public());
    Keyring::new(keys)
}

/// Hash of everything that determines the build's output.
fn build_stamp(recipe: &Recipe, cfg: &BuildConfig, keyring: &Keyring) -> Result<String> {
    let mut s = String::new();
    let _ = writeln!(s, "recipe {}", recipe.content_hash()?);
    let mode = mode_of(recipe, cfg);
    let _ = writeln!(s, "mode {mode:?} target {}", cfg.target);
    if let Some(h) = host_of(cfg, mode)? {
        let _ = writeln!(s, "host {}", h.id());
    }
    let _ = writeln!(s, "epoch {SOURCE_DATE_EPOCH}");
    for src in recipe.sources.iter().filter(|s| s.is_local()) {
        let _ = writeln!(
            s,
            "local {} {}",
            src.file_name(),
            tree_hash(&local_path(&cfg.repo_root, src)?)?
        );
    }
    let deps = sysroot_deps(recipe, cfg);
    if !deps.is_empty() {
        let tmp = tempfile::tempdir().at(std::env::temp_dir())?;
        let mut entries = Vec::new();
        for loc in dep_locations(cfg) {
            entries.extend(Repo::open(loc, keyring, tmp.path())?.index.packages);
        }
        for d in dep_closure(&entries, &deps) {
            let _ = writeln!(s, "dep {d}");
        }
    }
    Ok(hash::sha256_hex(s.as_bytes()))
}

/// The repo packages a set of build deps pulls in (transitively), as
/// sorted `name version sha256` lines. Follows names (and `provides`)
/// only: the local repo holds one version of each package.
/// The packages a sysroot for `roots` would contain: for each dep, the
/// newest entry that satisfies it (as the solver picks), then its deps.
/// Versions that no dep would select (an older `~bootstrap` build, or a
/// final build next to a pinned base package) do not enter the stamp.
fn dep_closure(index: &[repo::IndexEntry], roots: &[DepSpec]) -> Vec<String> {
    let mut todo: Vec<&DepSpec> = roots.iter().collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out = BTreeSet::new();
    while let Some(dep) = todo.pop() {
        if !seen.insert(dep.to_string()) {
            continue;
        }
        let by_name = index
            .iter()
            .filter(|e| dep.matches(&e.name, &e.pkg_version()))
            .max_by_key(|e| e.pkg_version());
        let chosen = by_name.or_else(|| {
            index
                .iter()
                .filter(|e| e.provides.iter().any(|p| p.name == dep.name))
                .max_by_key(|e| e.pkg_version())
        });
        if let Some(e) = chosen {
            out.insert(format!("{} {} {}", e.name, e.pkg_version(), e.sha256));
            todo.extend(&e.depends);
        }
    }
    out.into_iter().collect()
}

fn outputs_in_repo(recipe: &Recipe, cfg: &BuildConfig, keyring: &Keyring) -> Result<bool> {
    if !cfg.repo.join(repo::INDEX).exists() {
        return Ok(false);
    }
    let tmp = tempfile::tempdir().at(std::env::temp_dir())?;
    let r = Repo::open(Location::Local(cfg.repo.clone()), keyring, tmp.path())?;
    let v = recipe.pkg_version();
    Ok(recipe.package_names().iter().all(|n| {
        r.index
            .packages
            .iter()
            .any(|e| &e.name == n && e.pkg_version() == v)
    }))
}

fn prepare_sysroot(
    recipe: &Recipe,
    cfg: &BuildConfig,
    host: Option<&HostTools>,
    sysroot: &Path,
    on: &mut dyn FnMut(BuildEvent),
) -> Result<()> {
    let keydir = sysroot.join(ADMIN_KEY_DIR);
    fs::create_dir_all(&keydir).at(&keydir)?;
    for (i, k) in keyring_of(cfg).keys().iter().enumerate() {
        let p = keydir.join(format!("build-{i}.pub"));
        fs::write(&p, k.to_text()).at(&p)?;
    }
    let deps = sysroot_deps(recipe, cfg);
    if !deps.is_empty() {
        on(BuildEvent::InstallingDeps { count: deps.len() });
        let mut root = Root::open(sysroot)?;
        let req = Request {
            install: deps,
            ..Request::default()
        };
        let sources = Sources {
            repos: dep_locations(cfg),
            files: vec![],
        };
        tx::run(&mut root, &req, &sources, &mut |_| {})
            .map_err(|e| Error::Build(format!("installing build dependencies: {e}")))?;
    }
    let bin = sysroot.join("usr/bin");
    fs::create_dir_all(&bin).at(&bin)?;
    // Host builds mount sysroot/usr at /usr; scripts expect /usr/bin/env.
    if let Some(h) = host
        && fs::symlink_metadata(bin.join("env")).is_err()
    {
        std::os::unix::fs::symlink(&h.env, bin.join("env")).at(bin.join("env"))?;
    }
    // Native builds get a minimal /etc (read-only).
    let etc = sysroot.parent().unwrap_or(sysroot).join("etc");
    fs::create_dir_all(&etc).at(&etc)?;
    for (f, text) in [
        (
            "passwd",
            format!(
                "root:x:0:0:root:/root:/bin/sh\nbuilder:x:{SANDBOX_UID}:{SANDBOX_UID}:builder:/build/home:/bin/sh\nnobody:x:65534:65534:nobody:/:/bin/false\n"
            ),
        ),
        (
            "group",
            format!("root:x:0:\nbuilder:x:{SANDBOX_UID}:\nnobody:x:65534:\n"),
        ),
        (
            "hosts",
            "127.0.0.1 localhost spk-build\n::1 localhost\n".to_string(),
        ),
    ] {
        fs::write(etc.join(f), text).at(etc.join(f))?;
    }
    Ok(())
}

fn extract_script(fetched: &[(Source, PathBuf)]) -> String {
    let mut s = String::from("set -euo pipefail\n");
    for (src, _) in fetched {
        let f = crate::recipe::sh_quote(src.file_name());
        if src.extract {
            let _ = writeln!(
                s,
                "tar -xf /build/sources/{f} --no-same-owner --no-same-permissions"
            );
        } else {
            let _ = writeln!(s, "cp /build/sources/{f} .");
        }
    }
    s
}

/// The directory the build runs in, relative to `src`.
fn source_dir(src: &Path, sources: &[Source]) -> Result<String> {
    if let Some(d) = sources.first().and_then(|s| s.dir.clone()) {
        validate_rel(&d)?;
        return Ok(d);
    }
    let mut dirs = Vec::new();
    for e in fs::read_dir(src).at(src)? {
        let e = e.at(src)?;
        if e.file_type().at(e.path())?.is_dir() {
            dirs.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    match dirs.as_slice() {
        [one] => {
            validate_rel(one)?;
            Ok(one.clone())
        }
        _ => Err(Error::Build(format!(
            "sources unpacked into {} directories; set `dir` on the first [[source]]",
            dirs.len()
        ))),
    }
}

fn validate_rel(d: &str) -> Result<()> {
    if d.is_empty() || d.starts_with('/') || d.split('/').any(|c| c == ".." || c.is_empty()) {
        return Err(Error::Build(format!("bad source dir {d:?}")));
    }
    Ok(())
}

/// What one sandboxed run needs.
struct Sandbox<'a> {
    cfg: &'a BuildConfig,
    mode: ToolchainMode,
    host: Option<&'a HostTools>,
    sysroot: &'a Path,
    bdir: &'a Path,
    etc: &'a Path,
    /// Vendored crates, bound read-only at /build/vendor.
    vendor: Option<&'a Path>,
}

/// Runs `script` with bash inside the sandbox.
fn run_sandbox(
    sb: &Sandbox<'_>,
    chdir: &str,
    script: &str,
    run_uid: Option<u32>,
    log: &mut File,
    log_path: &Path,
) -> Result<()> {
    let args = sandbox_args(sb, chdir, script);
    // bwrap and setpriv are the *host's* sandboxing tools, whatever the
    // build's toolchain mode.
    let host_path = std::env::var("PATH").unwrap_or_default();
    let bwrap = which(&host_path, "bwrap").ok_or_else(|| Error::Build("bwrap not found".into()))?;
    let mut cmd = if let Some(uid) = run_uid {
        let setpriv =
            which(&host_path, "setpriv").ok_or_else(|| Error::Build("setpriv not found".into()))?;
        let mut c = Command::new(setpriv);
        c.args([
            &format!("--reuid={uid}"),
            &format!("--regid={uid}"),
            "--clear-groups",
            "--inh-caps=-all",
            "--bounding-set=-all",
            "--no-new-privs",
        ]);
        c.arg(&bwrap);
        c
    } else {
        Command::new(&bwrap)
    };
    cmd.args(&args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(log.try_clone().at(log_path)?)
        .stderr(log.try_clone().at(log_path)?);
    let status = cmd
        .status()
        .map_err(|e| Error::Build(format!("cannot start the build sandbox: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Build(format!(
            "build script failed ({status}); full log: {}\n{}",
            log_path.display(),
            log_tail(log_path)
        )))
    }
}

/// bubblewrap arguments: every namespace unshared (network included), no
/// capabilities, tmpfs root, read-only toolchain and sysroot, writable
/// `/build` only. `native` builds see no host files at all.
fn sandbox_args(sb: &Sandbox<'_>, chdir: &str, script: &str) -> Vec<String> {
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let uid = SANDBOX_UID.to_string();
    let mut args: Vec<String> = [
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
        "--hostname",
        "spk-build",
        "--uid",
        &uid,
        "--gid",
        &uid,
        "--tmpfs",
        "/",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut add = |xs: &[&str]| args.extend(xs.iter().map(ToString::to_string));
    if sb.host.is_some() {
        add(&["--ro-bind", "/nix/store", "/nix/store"]);
    }
    add(&[
        "--ro-bind",
        &s(&sb.sysroot.join("usr")),
        "/usr",
        "--bind",
        &s(sb.bdir),
        "/build",
        "--ro-bind",
        &s(&sb.bdir.join("recipe")),
        "/build/recipe",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
    ]);
    if let Some(v) = sb.vendor {
        add(&["--ro-bind", &s(v), "/build/vendor"]);
    }
    match (sb.mode, sb.host) {
        (ToolchainMode::Host | ToolchainMode::Cross, Some(h)) => add(&[
            "--dir",
            "/bin",
            "--symlink",
            &s(&h.bash),
            "/bin/sh",
            "--symlink",
            &s(&h.bash),
            "/bin/bash",
        ]),
        // Our own merged-/usr layout (CLAUDE.md section 4).
        _ => add(&[
            "--symlink",
            "usr/bin",
            "/bin",
            "--symlink",
            "usr/sbin",
            "/sbin",
            "--symlink",
            "usr/lib",
            "/lib",
            "--symlink",
            "usr/lib",
            "/lib64",
            "--ro-bind",
            &s(sb.etc),
            "/etc",
        ]),
    }
    add(&["--chdir", chdir, "--clearenv"]);

    for (k, v) in sandbox_env(sb) {
        args.extend(["--setenv".into(), k.into(), v]);
    }
    args.extend(["/bin/bash".into(), "-c".into(), script.to_string()]);
    args
}

/// The build's entire environment (the sandbox starts with `--clearenv`).
fn sandbox_env(sb: &Sandbox<'_>) -> Vec<(&'static str, String)> {
    let cfg = sb.cfg;
    let path = match (sb.mode, sb.host) {
        (ToolchainMode::Native, _) | (_, None) => "/usr/bin".to_string(),
        // Ours first, so cc/ld/as are our toolchain; then the host's Rust.
        (ToolchainMode::Rust, Some(h)) => format!("/usr/bin:/usr/sbin:{}", h.path),
        (ToolchainMode::Cross, Some(h)) => format!("/usr/cross/bin:{}:/usr/bin", h.path),
        (ToolchainMode::Host, Some(h)) => format!("{}:/usr/bin", h.path),
    };
    let mut env: Vec<(&'static str, String)> = vec![
        ("PATH", path),
        ("HOME", "/build/home".into()),
        ("TMPDIR", "/tmp".into()),
        ("DESTDIR", "/build/dest".into()),
        ("RECIPE", "/build/recipe".into()),
        ("MAKEFLAGS", format!("-j{}", cfg.jobs.max(1))),
        ("SOL_JOBS", cfg.jobs.max(1).to_string()),
        ("SOL_TGT", cfg.target.clone()),
        ("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH.to_string()),
        ("LC_ALL", "C".into()),
        ("TZ", "UTC".into()),
        ("TERM", "dumb".into()),
        ("SHELL", "/bin/sh".into()),
        ("CONFIG_SHELL", "/bin/sh".into()),
    ];
    if sb.mode == ToolchainMode::Rust {
        env.extend([
            ("CARGO_HOME", "/build/cargo-home".to_string()),
            ("CARGO_NET_OFFLINE", "true".to_string()),
            ("CARGO_TARGET_DIR", "/build/target".to_string()),
            // Static binaries against our glibc, linked by our gcc.
            (
                "RUSTFLAGS",
                "-C target-feature=+crt-static -C linker=cc".to_string(),
            ),
        ]);
    }
    if sb.mode == ToolchainMode::Host {
        // Phase 1 host toolchain: point it at the sysroot. Its loader does
        // not search /usr/lib, so test programs need an rpath.
        env.extend([
            ("CPPFLAGS", "-I/usr/include".to_string()),
            ("LDFLAGS", "-L/usr/lib -Wl,-rpath,/usr/lib".to_string()),
            (
                "PKG_CONFIG_PATH",
                "/usr/lib/pkgconfig:/usr/share/pkgconfig".to_string(),
            ),
            (
                "PKG_CONFIG_PATH_FOR_TARGET",
                "/usr/lib/pkgconfig:/usr/share/pkgconfig".to_string(),
            ),
            ("CMAKE_PREFIX_PATH", "/usr".to_string()),
        ]);
    }
    env
}

fn which(path: &str, exe: &str) -> Option<PathBuf> {
    path.split(':')
        .map(|d| Path::new(d).join(exe))
        .find(|p| p.is_file())
}

fn log_tail(p: &Path) -> String {
    let Ok(f) = File::open(p) else {
        return String::new();
    };
    let lines: Vec<String> = BufReader::new(f)
        .lines()
        .map_while(std::result::Result::ok)
        .collect();
    let start = lines.len().saturating_sub(LOG_TAIL_LINES);
    lines[start..].join("\n")
}

fn chown_tree(dir: &Path, uid: u32) -> Result<()> {
    for e in walkdir::WalkDir::new(dir).follow_links(false) {
        let e = e.map_err(|e| Error::Build(e.to_string()))?;
        std::os::unix::fs::lchown(e.path(), Some(uid), Some(uid)).at(e.path())?;
    }
    Ok(())
}

/// Builds can leave read-only directories behind; make them deletable.
fn make_removable(dir: &Path) {
    for e in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if e.file_type().is_dir() {
            let _ = fs::set_permissions(e.path(), fs::Permissions::from_mode(0o755));
        }
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for e in walkdir::WalkDir::new(src).follow_links(false).min_depth(1) {
        let e = e.map_err(|e| Error::Build(e.to_string()))?;
        let rel = e.path().strip_prefix(src).unwrap_or(e.path());
        let to = dst.join(rel);
        if e.file_type().is_dir() {
            fs::create_dir_all(&to).at(&to)?;
        } else if e.file_type().is_file() {
            fs::copy(e.path(), &to).at(&to)?;
        }
    }
    Ok(())
}

/// Is `p` a real directory (not a symlink to one)?
fn real_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
}

/// Ensures `dir` exists as a real directory, creating it if missing.
fn ensure_real_dir(dir: &Path) -> Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_dir() => Ok(()),
        Ok(_) => Err(Error::Build(format!(
            "{} is not a directory",
            dir.display()
        ))),
        Err(_) => fs::create_dir(dir).at(dir),
    }
}

/// Output normalisation (docs/formats/recipe.md). Never follows symlinks.
fn normalise_dest(dest: &Path) -> Result<()> {
    if !real_dir(dest) {
        return Err(Error::Build(
            "DESTDIR was replaced by something else".into(),
        ));
    }
    let etc = dest.join("etc");
    if fs::symlink_metadata(&etc).is_ok() {
        if !real_dir(&etc) {
            return Err(Error::Build(
                "etc/ in the build output is not a directory".into(),
            ));
        }
        for d in ["usr", "usr/share", "usr/share/factory"] {
            ensure_real_dir(&dest.join(d))?;
        }
        let target = dest.join("usr/share/factory/etc");
        if fs::symlink_metadata(&target).is_ok() {
            return Err(Error::Build(
                "both etc/ and usr/share/factory/etc exist".into(),
            ));
        }
        fs::rename(&etc, &target).at(&target)?;
    }
    // Empty directories outside usr/ (e.g. var/lib/foo) are dropped;
    // anything with content is an error.
    for e in fs::read_dir(dest).at(dest)? {
        let e = e.at(dest)?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "usr" {
            if !real_dir(&e.path()) {
                return Err(Error::Build(
                    "usr/ in the build output is not a directory".into(),
                ));
            }
            continue;
        }
        let has_files = walkdir::WalkDir::new(e.path())
            .follow_links(false)
            .into_iter()
            .flatten()
            .any(|x| !x.file_type().is_dir());
        if has_files {
            return Err(Error::Build(format!(
                "the build installed files into /{name}; everything must be under /usr \
                 (configure --prefix=/usr, or move them in post_install)"
            )));
        }
        make_removable(&e.path());
        fs::remove_dir_all(e.path()).at(e.path())?;
    }
    // Libtool archives and the info index are never shipped.
    for e in walkdir::WalkDir::new(dest.join("usr"))
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        let n = e.file_name().to_string_lossy();
        let is_la = e.file_type().is_file() && n.ends_with(".la");
        let is_info_dir = e.path().ends_with("usr/share/info/dir");
        if is_la || is_info_dir {
            fs::remove_file(e.path()).at(e.path())?;
        }
    }
    Ok(())
}

/// Every path under `dest` (relative, sorted), directories included.
fn list_dest(dest: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(dest)
        .follow_links(false)
        .min_depth(1)
        .sort_by_file_name()
    {
        let e = e.map_err(|e| Error::Build(e.to_string()))?;
        let rel = e
            .path()
            .strip_prefix(dest)
            .ok()
            .and_then(Path::to_str)
            .ok_or_else(|| Error::Build(format!("non-UTF-8 path {}", e.path().display())))?
            .to_string();
        validate_payload_path(&rel).map_err(|err| Error::Build(err.to_string()))?;
        let ft = e.file_type();
        if !(ft.is_dir() || ft.is_file() || ft.is_symlink()) {
            return Err(Error::Build(format!("{rel}: unsupported file type")));
        }
        out.push(rel);
    }
    if out.is_empty() {
        return Err(Error::Build("the build installed nothing".into()));
    }
    Ok(out)
}

fn parents(p: &str) -> impl Iterator<Item = &str> {
    p.match_indices('/').map(move |(i, _)| &p[..i])
}

/// Assigns each path to a package: split globs first (in order), the rest
/// to the main package. Every package gets the directories above its files.
fn split_paths(recipe: &Recipe, all: &[String]) -> Result<Vec<(String, Vec<String>)>> {
    let opts = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let mut owner: HashMap<&str, usize> = HashMap::new(); // path -> split idx + 1
    for (i, s) in recipe.splits.iter().enumerate() {
        let pats: Vec<glob::Pattern> = s
            .files
            .iter()
            .map(|g| glob::Pattern::new(g).map_err(|e| Error::invalid(e.to_string())))
            .collect::<Result<_>>()?;
        let mut matched = false;
        for p in all {
            if !owner.contains_key(p.as_str()) && pats.iter().any(|g| g.matches_with(p, opts)) {
                owner.insert(p, i + 1);
                matched = true;
            }
        }
        if !matched {
            return Err(Error::Build(format!("split {} matched no files", s.name)));
        }
    }
    let mut groups: Vec<BTreeSet<&str>> = vec![BTreeSet::new(); recipe.splits.len() + 1];
    for p in all {
        let g = owner.get(p.as_str()).copied().unwrap_or(0);
        groups[g].insert(p);
    }
    // A directory claimed by a split brings its whole subtree along.
    for p in all {
        if let Some(g) = parents(p).find_map(|d| owner.get(d).copied())
            && g != 0
        {
            groups[0].remove(p.as_str());
            groups[g].insert(p);
        }
    }
    let mut out = Vec::new();
    for (i, set) in groups.into_iter().enumerate() {
        let mut paths: BTreeSet<&str> = set.clone();
        for p in &set {
            for d in parents(p) {
                paths.insert(d);
            }
        }
        let name = if i == 0 {
            recipe.name.clone()
        } else {
            recipe.splits[i - 1].name.clone()
        };
        out.push((name, paths.into_iter().map(String::from).collect()));
    }
    Ok(out)
}

/// Pins unversioned deps on this recipe's own packages to the exact
/// version being built.
fn pin_siblings(deps: &[DepSpec], recipe: &Recipe) -> Vec<DepSpec> {
    let siblings = recipe.package_names();
    deps.iter()
        .map(|d| {
            if d.constraint.is_none() && siblings.contains(&d.name) {
                DepSpec {
                    name: d.name.clone(),
                    constraint: Some(Constraint {
                        op: Op::Eq,
                        version: recipe.version.clone(),
                        release: Some(recipe.release),
                    }),
                }
            } else {
                d.clone()
            }
        })
        .collect()
}

fn package_meta(recipe: &Recipe, name: &str, recipe_sha: &str) -> PackageMeta {
    let split = recipe.splits.iter().find(|s| s.name == name);
    let (summary, depends, provides, conflicts, replaces) = match split {
        Some(s) => (
            s.summary.clone(),
            pin_siblings(&s.depends, recipe),
            s.provides.clone(),
            Vec::new(),
            Vec::new(),
        ),
        None => (
            recipe.summary.clone(),
            pin_siblings(&recipe.deps.runtime, recipe),
            recipe.package.provides.clone(),
            recipe.package.conflicts.clone(),
            recipe.package.replaces.clone(),
        ),
    };
    PackageMeta {
        name: name.to_string(),
        version: recipe.version.clone(),
        release: recipe.release,
        summary,
        license: recipe.license.clone(),
        depends,
        provides,
        conflicts,
        replaces,
        build_date: SOURCE_DATE_EPOCH,
        recipe_sha256: recipe_sha.to_string(),
    }
}

/// Loads every recipe under `dir` (sorted by name).
///
/// # Errors
/// Any recipe that fails to load or validate.
pub fn load_recipes(dir: &Path) -> Result<Vec<Recipe>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).at(dir)? {
        let e = e.at(dir)?;
        if e.path().join("recipe.toml").is_file() {
            out.push(Recipe::load(&e.path())?);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Depth-first post-order walk for [`build_order`].
fn visit<'a>(
    r: &'a Recipe,
    producer: &HashMap<String, &'a Recipe>,
    state: &mut HashMap<&'a str, u8>,
    order: &mut Vec<&'a Recipe>,
    chain: &mut Vec<&'a str>,
) -> Result<()> {
    match state.get(r.name.as_str()) {
        Some(2) => return Ok(()),
        Some(1) => {
            chain.push(&r.name);
            return Err(Error::invalid(format!(
                "build dependency cycle: {}",
                chain.join(" -> ")
            )));
        }
        _ => {}
    }
    state.insert(&r.name, 1);
    chain.push(&r.name);
    for d in r.deps.build.iter().chain(&r.deps.runtime) {
        let Some(dep) = producer.get(d.name.as_str()) else {
            return Err(Error::invalid(format!(
                "{} depends on {}, which no recipe produces",
                r.name, d.name
            )));
        };
        if dep.name != r.name {
            visit(dep, producer, state, order, chain)?;
        }
    }
    chain.pop();
    state.insert(&r.name, 2);
    order.push(r);
    Ok(())
}

/// Orders `wanted` (and the recipes they need to build) so every recipe
/// comes after the recipes producing its build dependencies.
///
/// # Errors
/// Unknown build dependencies and dependency cycles.
pub fn build_order<'a>(all: &'a [Recipe], wanted: &[String]) -> Result<Vec<&'a Recipe>> {
    let mut producer: HashMap<String, &Recipe> = HashMap::new();
    for r in all {
        for n in r.package_names() {
            if let Some(prev) = producer.insert(n, r) {
                return Err(Error::invalid(format!(
                    "package produced by two recipes: {} and {}",
                    prev.name, r.name
                )));
            }
        }
    }
    let mut order = Vec::new();
    let mut state: HashMap<&str, u8> = HashMap::new(); // 1 = visiting, 2 = done
    for w in wanted {
        let r = all
            .iter()
            .find(|r| &r.name == w)
            .ok_or_else(|| Error::invalid(format!("no recipe named {w}")))?;
        visit(r, &producer, &mut state, &mut order, &mut Vec::new())?;
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(name: &str, build: &[&str], splits: &str) -> Recipe {
        let text = format!(
            r#"
name = "{name}"
version = "1.0"
release = 1
summary = "s"
license = "MIT"
[[source]]
url = "https://example.org/{name}.tar.gz"
sha256 = "{}"
[deps]
build = [{}]
[build]
system = "make"
{splits}
"#,
            "0".repeat(64),
            build
                .iter()
                .map(|b| format!("\"{b}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut r: Recipe = toml::from_str(&text).unwrap();
        r.dir = PathBuf::from(format!("recipes/{name}"));
        r.validate().unwrap();
        r
    }

    #[test]
    fn order_respects_build_deps() {
        let all = vec![
            recipe("app", &["libz", "xz"], ""),
            recipe(
                "xz",
                &["libz"],
                "[[split]]\nname = \"liblzma\"\nsummary = \"l\"\nfiles = [\"usr/lib/*\"]",
            ),
            recipe("libz", &[], ""),
            recipe("other", &["liblzma"], ""),
        ];
        let names = |v: Vec<&Recipe>| v.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(build_order(&all, &["app".into()]).unwrap()),
            ["libz", "xz", "app"]
        );
        assert_eq!(
            names(build_order(&all, &["other".into()]).unwrap()),
            ["libz", "xz", "other"]
        );
    }

    #[test]
    fn order_detects_cycles_and_unknowns() {
        let all = vec![recipe("a", &["b"], ""), recipe("b", &["a"], "")];
        assert!(
            build_order(&all, &["a".into()])
                .unwrap_err()
                .to_string()
                .contains("cycle")
        );
        let all = vec![recipe("a", &["nope"], "")];
        assert!(build_order(&all, &["a".into()]).is_err());
    }

    #[test]
    fn splitting() {
        let r = recipe(
            "xz",
            &[],
            "[[split]]\nname = \"liblzma\"\nsummary = \"l\"\nfiles = [\"usr/lib/liblzma.so*\", \"usr/include\"]",
        );
        let all: Vec<String> = [
            "usr",
            "usr/bin",
            "usr/bin/xz",
            "usr/include",
            "usr/include/lzma.h",
            "usr/include/lzma",
            "usr/include/lzma/a.h",
            "usr/lib",
            "usr/lib/liblzma.so",
            "usr/lib/liblzma.so.5",
            "usr/lib/sub",
            "usr/lib/sub/liblzma.so.x",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let g = split_paths(&r, &all).unwrap();
        assert_eq!(g[0].0, "xz");
        assert_eq!(
            g[0].1,
            [
                "usr",
                "usr/bin",
                "usr/bin/xz",
                "usr/lib",
                "usr/lib/sub",
                "usr/lib/sub/liblzma.so.x"
            ]
        );
        assert_eq!(g[1].0, "liblzma");
        assert_eq!(
            g[1].1,
            [
                "usr",
                "usr/include",
                "usr/include/lzma",
                "usr/include/lzma.h",
                "usr/include/lzma/a.h",
                "usr/lib",
                "usr/lib/liblzma.so",
                "usr/lib/liblzma.so.5"
            ]
        );
    }

    #[test]
    fn stamp_closure_is_transitive_and_ignores_unrelated() {
        let entry = |name: &str, deps: &[&str]| repo::IndexEntry {
            name: name.into(),
            version: crate::version::Version::parse("1").unwrap(),
            release: 1,
            arch: "x86_64".into(),
            summary: String::new(),
            depends: deps.iter().map(|d| DepSpec::parse(d).unwrap()).collect(),
            provides: vec![],
            conflicts: vec![],
            replaces: vec![],
            file: String::new(),
            size: 0,
            sha256: name.repeat(2),
            installed_size: 0,
        };
        let idx = vec![
            entry("a", &["b"]),
            entry("b", &["c"]),
            entry("c", &[]),
            entry("unrelated", &[]),
        ];
        let got = dep_closure(&idx, &[DepSpec::parse("a").unwrap()]);
        assert_eq!(got, ["a 1-1 aa", "b 1-1 bb", "c 1-1 cc"]);
    }

    #[test]
    fn stamp_closure_follows_constraints_not_every_version() {
        let entry = |name: &str, ver: &str| repo::IndexEntry {
            name: name.into(),
            version: crate::version::Version::parse(ver).unwrap(),
            release: 1,
            arch: "x86_64".into(),
            summary: String::new(),
            depends: vec![],
            provides: vec![],
            conflicts: vec![],
            replaces: vec![],
            file: String::new(),
            size: 0,
            sha256: format!("{name}{ver}"),
            installed_size: 0,
        };
        let idx = vec![entry("bash", "5.3~bootstrap"), entry("bash", "5.3")];
        // A pinned base package: building the final bash must not change it.
        let pinned = dep_closure(&idx, &[DepSpec::parse("bash=5.3~bootstrap-1").unwrap()]);
        assert_eq!(pinned, ["bash 5.3~bootstrap-1 bash5.3~bootstrap"]);
        // Unpinned: the newest, as the solver picks.
        let newest = dep_closure(&idx, &[DepSpec::parse("bash").unwrap()]);
        assert_eq!(newest, ["bash 5.3-1 bash5.3"]);
    }

    #[test]
    fn sibling_deps_are_pinned() {
        let mut r = recipe(
            "xz",
            &[],
            "[[split]]\nname = \"liblzma\"\nsummary = \"l\"\nfiles = [\"usr/lib/*\"]",
        );
        r.deps.runtime = vec![
            DepSpec::parse("liblzma").unwrap(),
            DepSpec::parse("zlib").unwrap(),
        ];
        let m = package_meta(&r, "xz", &"0".repeat(64));
        assert_eq!(m.depends[0].to_string(), "liblzma=1.0-1");
        assert_eq!(m.depends[1].to_string(), "zlib");
    }

    #[test]
    fn normalisation_moves_etc_and_rejects_strays() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path();
        fs::create_dir_all(dest.join("etc/x")).unwrap();
        fs::write(dest.join("etc/x/a.conf"), "a").unwrap();
        fs::create_dir_all(dest.join("usr/lib")).unwrap();
        fs::write(dest.join("usr/lib/libz.la"), "la").unwrap();
        fs::create_dir_all(dest.join("var/lib/empty")).unwrap();
        normalise_dest(dest).unwrap();
        assert!(dest.join("usr/share/factory/etc/x/a.conf").is_file());
        assert!(!dest.join("etc").exists());
        assert!(!dest.join("var").exists());
        assert!(!dest.join("usr/lib/libz.la").exists());

        fs::create_dir_all(dest.join("opt")).unwrap();
        fs::write(dest.join("opt/x"), "x").unwrap();
        assert!(
            normalise_dest(dest)
                .unwrap_err()
                .to_string()
                .contains("/opt")
        );
    }

    #[test]
    fn normalisation_refuses_symlinked_usr() {
        // A hostile build replaces usr/ with a link to the host: spk must not
        // move etc/ through it.
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let dest = d.path();
        fs::create_dir_all(dest.join("etc")).unwrap();
        fs::write(dest.join("etc/a"), "a").unwrap();
        std::os::unix::fs::symlink(outside.path(), dest.join("usr")).unwrap();
        assert!(normalise_dest(dest).is_err());
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn listing_does_not_follow_symlinks() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "s").unwrap();
        fs::create_dir_all(d.path().join("usr")).unwrap();
        std::os::unix::fs::symlink(outside.path(), d.path().join("usr/lib")).unwrap();
        let all = list_dest(d.path()).unwrap();
        assert_eq!(
            all,
            ["usr", "usr/lib"],
            "the link is listed, not its target's contents"
        );
    }

    #[test]
    fn a_split_build_dep_swaps_out_its_base_parent() {
        let spec = |s: &str| DepSpec::parse(s).unwrap();
        let base = [spec("xz=5.8.4~bootstrap-1"), spec("bash=5.3~bootstrap-1")];
        let stage = StageGraph {
            split_parents: HashMap::from([("liblzma".to_string(), "xz".to_string())]),
            runtime: HashMap::from([
                (
                    "elfutils".to_string(),
                    vec!["liblzma".to_string(), "zlib".to_string()],
                ),
                ("liblzma".to_string(), Vec::new()),
            ]),
        };
        let names = |d: &[DepSpec]| d.iter().map(ToString::to_string).collect::<Vec<_>>();
        // liblzma comes from xz: the stage's own xz, not the base one.
        assert_eq!(
            names(&merge_base(&[spec("liblzma")], &base, &stage)),
            ["liblzma", "xz", "bash=5.3~bootstrap-1"]
        );
        // Also when liblzma only comes in at runtime (elfutils needs it).
        assert_eq!(
            names(&merge_base(&[spec("elfutils")], &base, &stage)),
            ["elfutils", "xz", "bash=5.3~bootstrap-1"]
        );
        // A base package the recipe names itself is not added twice.
        assert_eq!(
            names(&merge_base(&[spec("bash")], &base, &stage)),
            ["bash", "xz=5.8.4~bootstrap-1"]
        );
        // Unrelated deps keep the pinned base.
        assert_eq!(
            names(&merge_base(&[spec("zlib")], &base, &stage)),
            ["zlib", "xz=5.8.4~bootstrap-1", "bash=5.3~bootstrap-1"]
        );
    }

    #[test]
    fn base_deps_pin_to_the_newest_dependency_repo_version() {
        let entry = |name: &str, ver: &str, release: u32| repo::IndexEntry {
            name: name.into(),
            version: crate::version::Version::parse(ver).unwrap(),
            release,
            arch: "x86_64".into(),
            summary: String::new(),
            depends: Vec::new(),
            provides: Vec::new(),
            conflicts: Vec::new(),
            replaces: Vec::new(),
            file: format!("{name}.solpkg"),
            size: 1,
            sha256: "0".repeat(64),
            installed_size: 1,
        };
        let entries = [
            entry("bash", "5.2~bootstrap", 1),
            entry("bash", "5.3~bootstrap", 2),
            entry("gcc", "16.2.0~bootstrap", 1),
        ];
        let mut base = vec![
            DepSpec::parse("bash").unwrap(),
            DepSpec::parse("gcc>=1").unwrap(),
        ];
        pin_to_newest(&mut base, &entries).unwrap();
        assert_eq!(base[0].to_string(), "bash=5.3~bootstrap-2");
        // An explicit constraint is left alone.
        assert_eq!(base[1].to_string(), "gcc>=1");
        let mut missing = vec![DepSpec::parse("perl").unwrap()];
        assert!(pin_to_newest(&mut missing, &entries).is_err());
    }

    #[test]
    fn local_sources_stay_inside_the_repo() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join("code/target")).unwrap();
        fs::write(repo.path().join("code/a.rs"), "a").unwrap();
        fs::write(repo.path().join("code/target/junk"), "j").unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join("escape")).unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join("code/link")).unwrap();
        let src = |p: &str| Source {
            url: String::new(),
            sha256: String::new(),
            mirrors: vec![],
            extract: false,
            dir: None,
            local: Some(p.into()),
        };
        assert!(local_path(repo.path(), &src("escape")).is_err());
        let code = local_path(repo.path(), &src("code")).unwrap();
        let dst = tempfile::tempdir().unwrap();
        copy_tree(&code, &dst.path().join("code")).unwrap();
        assert!(dst.path().join("code/a.rs").is_file());
        assert!(
            !dst.path().join("code/target").exists(),
            "build outputs skipped"
        );
        assert!(
            !dst.path().join("code/link").exists(),
            "symlinks not copied"
        );
        let h1 = tree_hash(&code).unwrap();
        fs::write(repo.path().join("code/a.rs"), "b").unwrap();
        assert_ne!(h1, tree_hash(&code).unwrap());
    }

    #[test]
    fn scripts_quote_file_names() {
        let src = Source {
            url: "https://e.org/a b;rm -rf.tar.gz".into(),
            sha256: "0".repeat(64),
            mirrors: vec![],
            extract: true,
            dir: None,
            local: None,
        };
        let s = extract_script(&[(src, PathBuf::new())]);
        assert!(s.contains("'a b;rm -rf.tar.gz'"), "{s}");
    }

    #[test]
    fn source_dir_detection() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("zlib-1.3.1")).unwrap();
        assert_eq!(source_dir(d.path(), &[]).unwrap(), "zlib-1.3.1");
        fs::create_dir(d.path().join("other")).unwrap();
        assert!(source_dir(d.path(), &[]).is_err());
    }
}
