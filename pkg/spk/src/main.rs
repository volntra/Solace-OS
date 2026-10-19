//! spk: the Solace package manager.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use libspk::bootstrap::Plan;
use libspk::build::{self, BuildConfig, BuildEvent, HostTools, ToolchainMode};
use libspk::db::Db;
use libspk::fetch::Location;
use libspk::manifest::{FileKind, Manifest};
use libspk::package::VerifiedPackage;
use libspk::repo::Repo;
use libspk::sign::{ADMIN_KEY_DIR, Keyring, PublicKey, SecretKey};
use libspk::solve::Request;
use libspk::tx::{self, Event, Root, Sources};
use libspk::version::DepSpec;
use owo_colors::OwoColorize;

#[derive(Parser)]
#[command(name = "spk", version, about = "The Solace package manager")]
struct Cli {
    /// Operate on this root instead of /
    #[arg(long, global = true, default_value = "/")]
    root: PathBuf,
    /// Extra repository (https:// URL or local path); repeatable
    #[arg(long = "repo", global = true)]
    repos: Vec<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Install packages (names, specs like `zlib>=1.3`, or .solpkg files)
    Install {
        #[arg(required = true)]
        packages: Vec<String>,
    },
    /// Remove packages
    Remove {
        #[arg(required = true)]
        packages: Vec<String>,
    },
    /// Upgrade every installed package to the newest available version
    Upgrade,
    /// Search repositories by name and summary
    Search { term: String },
    /// Show details of an installed package, a repository package, or a file
    Info { package: String },
    /// List the files of an installed package
    Files { package: String },
    /// Show which package owns a path
    Owns { path: String },
    /// List installed packages
    List,
    /// Show the transaction history
    History,
    /// Check installed files against the package database
    Verify,
    /// Make a previous system snapshot the default (Phase 3)
    Rollback,
    /// Build recipes into signed packages in a local repository
    Build(BuildArgs),
    /// Build the whole system from source, stage by stage (bootstrap/plan.toml)
    Bootstrap(BootstrapArgs),
    /// Add signed packages to a repository and re-sign its index
    RepoAdd {
        repo: PathBuf,
        #[arg(required = true)]
        packages: Vec<PathBuf>,
        /// Signing key (.key)
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "solace")]
        name: String,
    },
    /// Manage signing keys
    #[command(subcommand)]
    Key(KeyCmd),
}

#[derive(clap::Args)]
struct BuildOpts {
    #[arg(long, default_value = "build/work")]
    work: PathBuf,
    #[arg(long, default_value = "build/sources")]
    sources: PathBuf,
    /// Signing key (.key)
    #[arg(long, default_value = "build/keys/dev.key")]
    key: PathBuf,
    /// Parallel jobs inside each build
    #[arg(short, long)]
    jobs: Option<usize>,
    /// Rebuild even if nothing changed
    #[arg(long)]
    force: bool,
    /// Keep each build's work tree after success (can be several GB)
    #[arg(long)]
    keep_work: bool,
}

#[derive(clap::Args)]
struct BuildArgs {
    /// Recipe names (or recipe directories)
    recipes: Vec<String>,
    /// Build every recipe in --recipes-dir
    #[arg(long)]
    all: bool,
    #[arg(long, default_value = "recipes")]
    recipes_dir: PathBuf,
    /// Local repository that receives the packages
    #[arg(long, default_value = "build/repo")]
    repo_dir: PathBuf,
    /// Further repositories build deps may come from; repeatable
    #[arg(long = "dep-repo")]
    dep_repos: Vec<PathBuf>,
    /// Toolchain for every recipe: host, cross, or native (default: each
    /// recipe's own, else native)
    #[arg(long, value_parser = parse_mode)]
    toolchain: Option<ToolchainMode>,
    /// Packages installed into every native sysroot; repeatable
    #[arg(long = "base")]
    base: Vec<String>,
    #[command(flatten)]
    opts: BuildOpts,
}

#[derive(clap::Args)]
struct BootstrapArgs {
    /// Bootstrap plan
    #[arg(default_value = "bootstrap/plan.toml")]
    plan: PathBuf,
    /// Run only these stages (repeatable); default: all, in order
    #[arg(long = "stage")]
    stages: Vec<String>,
    /// Stop after this recipe (useful while writing recipes)
    #[arg(long)]
    until: Option<String>,
    #[command(flatten)]
    opts: BuildOpts,
}

fn parse_mode(s: &str) -> std::result::Result<ToolchainMode, String> {
    match s {
        "host" => Ok(ToolchainMode::Host),
        "cross" => Ok(ToolchainMode::Cross),
        "native" => Ok(ToolchainMode::Native),
        _ => Err(format!("unknown toolchain {s:?} (host, cross, native)")),
    }
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Generate a signing key pair (<dir>/<name>.key and .pub)
    Gen {
        name: String,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Trust a public key for installs into --root
    Trust { pubkey: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            anstream::eprintln!("{} {e:#}", "error:".red().bold());
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let repos = cli
        .repos
        .iter()
        .map(|r| Location::parse(r))
        .collect::<libspk::Result<Vec<_>>>()?;
    match cli.cmd {
        Cmd::Install { packages } => {
            let mut req = Request::default();
            let mut files = Vec::new();
            for p in packages {
                if p.ends_with(".solpkg") {
                    files.push(PathBuf::from(p));
                } else {
                    req.install.push(DepSpec::parse(&p)?);
                }
            }
            // Local files are requested by name once verified (in tx).
            let names = local_names(&cli.root, &files)?;
            req.install.extend(names);
            transact(&cli.root, &req, &Sources { repos, files })
        }
        Cmd::Remove { packages } => {
            let req = Request {
                remove: packages,
                ..Request::default()
            };
            transact(&cli.root, &req, &Sources::default())
        }
        Cmd::Upgrade => {
            let req = Request {
                upgrade: true,
                ..Request::default()
            };
            transact(
                &cli.root,
                &req,
                &Sources {
                    repos,
                    files: vec![],
                },
            )
        }
        Cmd::Search { term } => search(&cli.root, &repos, &term),
        Cmd::Info { package } => info(&cli.root, &repos, &package),
        Cmd::Files { package } => files(&cli.root, &package),
        Cmd::Owns { path } => owns(&cli.root, &path),
        Cmd::List => list(&cli.root),
        Cmd::History => history(&cli.root),
        Cmd::Verify => verify(&cli.root),
        Cmd::Rollback => bail!(
            "rollback switches between system snapshots, which arrive in Phase 3 \
             (not implemented yet)"
        ),
        Cmd::Build(args) => build_cmd(args),
        Cmd::Bootstrap(args) => bootstrap_cmd(&args),
        Cmd::RepoAdd {
            repo,
            packages,
            key,
            name,
        } => {
            let key = SecretKey::load(&key)?;
            let ring = Keyring::new(vec![key.public()]);
            let idx = libspk::repo::repo_add(&repo, &name, &packages, &key, &ring)?;
            anstream::println!(
                "{} {} now lists {} package(s)",
                "✓".green(),
                repo.display(),
                idx.packages.len()
            );
            Ok(())
        }
        Cmd::Key(k) => key_cmd(&cli.root, k),
    }
}

fn key_cmd(root: &Path, cmd: KeyCmd) -> Result<()> {
    match cmd {
        KeyCmd::Gen { name, dir } => {
            let k = SecretKey::generate()?;
            let (sk, pk) = k.save(&dir, &name)?;
            anstream::println!(
                "{} key {} written to {} (secret) and {}",
                "✓".green(),
                k.key_id(),
                sk.display(),
                pk.display()
            );
            Ok(())
        }
        KeyCmd::Trust { pubkey } => {
            guard_host_root(root)?;
            let text = std::fs::read_to_string(&pubkey)
                .with_context(|| format!("reading {}", pubkey.display()))?;
            let k = PublicKey::parse(&text)?;
            let dir = root.join(ADMIN_KEY_DIR);
            std::fs::create_dir_all(&dir)?;
            let dest = dir.join(format!("{}.pub", k.key_id()));
            std::fs::write(&dest, k.to_text())?;
            anstream::println!(
                "{} {} trusts key {}",
                "✓".green(),
                root.display(),
                k.key_id()
            );
            Ok(())
        }
    }
}

/// Reads the package names out of local files (verified) so they are
/// requested explicitly.
fn local_names(root: &Path, files: &[PathBuf]) -> Result<Vec<DepSpec>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let ring = Keyring::load(root)?;
    let tmp = tempfile::tempdir()?;
    files
        .iter()
        .map(|f| {
            let vp = VerifiedPackage::open(f, &ring, None, tmp.path())?;
            Ok(DepSpec::parse(&format!(
                "{}={}",
                vp.manifest.name,
                vp.manifest.pkg_version()
            ))?)
        })
        .collect()
}

fn bar_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:>12.cyan.bold} [{bar:30}] {pos}/{len} {msg}")
        .expect("valid template")
        .progress_chars("=> ")
}

fn spinner(prefix: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{prefix:>12.cyan.bold} {spinner} {msg}")
            .expect("valid template"),
    );
    pb.set_prefix(prefix.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Refuses to modify the host's `/` unless the host is Solace: during
/// development spk runs on other systems, where `--root /` would write into
/// the host's own /usr.
fn guard_host_root(root: &Path) -> Result<()> {
    let is_slash = root.canonicalize().is_ok_and(|p| p == Path::new("/"));
    if !is_slash {
        return Ok(());
    }
    let os = std::fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    if os.lines().any(|l| l.trim() == "ID=solace") {
        return Ok(());
    }
    bail!(
        "refusing to modify / on a system that is not Solace; pass --root <dir> \
         to work on a separate root"
    )
}

fn transact(root_path: &Path, req: &Request, sources: &Sources) -> Result<()> {
    guard_host_root(root_path)?;
    std::fs::create_dir_all(root_path)
        .with_context(|| format!("creating {}", root_path.display()))?;
    let mut root = Root::open(root_path)?;
    let mut fetch_bar: Option<ProgressBar> = None;
    let mut spin: Option<ProgressBar> = Some(spinner("Resolving"));
    let plan = tx::run(&mut root, req, sources, &mut |ev| match ev {
        Event::Resolving => {}
        Event::Planned { install, remove } => {
            if let Some(s) = spin.take() {
                s.finish_and_clear();
            }
            for i in &install {
                anstream::println!("  {} {i}", "+".green().bold());
            }
            for r in &remove {
                anstream::println!("  {} {r}", "-".red().bold());
            }
        }
        Event::Fetching { name, index, total } => {
            let b = fetch_bar.get_or_insert_with(|| {
                let b = ProgressBar::new(total as u64);
                b.set_style(bar_style());
                b.set_prefix("Verifying");
                b
            });
            b.set_position(index as u64);
            b.set_message(name);
        }
        Event::Extracting { name } => {
            if let Some(b) = fetch_bar.take() {
                b.finish_and_clear();
            }
            let s = spin.get_or_insert_with(|| spinner("Unpacking"));
            s.set_message(name);
        }
        Event::Applying => {
            if let Some(s) = spin.take() {
                s.finish_and_clear();
            }
            spin = Some(spinner("Applying"));
        }
        Event::Done => {
            if let Some(s) = spin.take() {
                s.finish_and_clear();
            }
        }
    });
    if let Some(s) = spin.take() {
        s.finish_and_clear();
    }
    let plan = plan?;
    if plan.install.is_empty() && plan.remove.is_empty() {
        anstream::println!("{} nothing to do", "✓".green());
    } else {
        anstream::println!(
            "{} {} installed, {} removed",
            "✓".green(),
            plan.install.len(),
            plan.remove.len()
        );
    }
    Ok(())
}

fn open_db(root: &Path) -> Result<Db> {
    if !root.join(libspk::db::DB_PATH).exists() {
        bail!(
            "{} has no package database (nothing installed)",
            root.display()
        );
    }
    Ok(Db::open(root)?)
}

fn open_repos(root: &Path, repos: &[Location]) -> Result<(Vec<Repo>, tempfile::TempDir)> {
    let ring = Keyring::load(root)?;
    let tmp = tempfile::tempdir()?;
    let opened = repos
        .iter()
        .map(|l| Repo::open(l.clone(), &ring, tmp.path()))
        .collect::<libspk::Result<Vec<_>>>()?;
    Ok((opened, tmp))
}

fn search(root: &Path, repos: &[Location], term: &str) -> Result<()> {
    if repos.is_empty() {
        bail!("no repositories given (use --repo)");
    }
    let installed = if root.join(libspk::db::DB_PATH).exists() {
        Db::open(root)?.installed()?
    } else {
        Vec::new()
    };
    let (repos, _tmp) = open_repos(root, repos)?;
    let t = term.to_lowercase();
    for r in &repos {
        for e in &r.index.packages {
            if e.name.contains(&t) || e.summary.to_lowercase().contains(&t) {
                let mark = if installed.iter().any(|i| i.manifest.name == e.name) {
                    " [installed]".green().to_string()
                } else {
                    String::new()
                };
                anstream::println!(
                    "{} {}{mark}\n    {}",
                    e.name.bold(),
                    e.pkg_version(),
                    e.summary
                );
            }
        }
    }
    Ok(())
}

fn print_manifest(m: &Manifest, extra: &[(&str, String)]) {
    let list = |v: &[DepSpec]| {
        if v.is_empty() {
            "-".to_string()
        } else {
            v.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let mut rows: Vec<(&str, String)> = vec![
        ("Name", m.name.clone()),
        ("Version", m.pkg_version().to_string()),
        ("Summary", m.summary.clone()),
        ("License", m.license.clone()),
        ("Depends", list(&m.depends)),
        ("Provides", list(&m.provides)),
        ("Conflicts", list(&m.conflicts)),
        ("Replaces", list(&m.replaces)),
        ("Size", format!("{} KiB", m.installed_size.div_ceil(1024))),
        (
            "Files",
            m.files
                .iter()
                .filter(|f| f.kind != FileKind::Dir)
                .count()
                .to_string(),
        ),
        ("Recipe", m.recipe_sha256[..16].to_string()),
    ];
    rows.extend(extra.iter().map(|(k, v)| (*k, v.clone())));
    for (k, v) in rows {
        anstream::println!("{:>10}  {v}", k.bold());
    }
}

fn info(root: &Path, repos: &[Location], what: &str) -> Result<()> {
    if what.ends_with(".solpkg") {
        let ring = Keyring::load(root)?;
        let tmp = tempfile::tempdir()?;
        let vp = VerifiedPackage::open(Path::new(what), &ring, None, tmp.path())?;
        print_manifest(&vp.manifest, &[("Signature", "verified".into())]);
        return Ok(());
    }
    if root.join(libspk::db::DB_PATH).exists()
        && let Some(i) = Db::open(root)?.get(what)?
    {
        let reason = if i.explicit {
            "explicitly"
        } else {
            "as a dependency"
        };
        print_manifest(&i.manifest, &[("Installed", reason.into())]);
        return Ok(());
    }
    let (repos, _tmp) = open_repos(root, repos)?;
    for r in &repos {
        if let Some(e) = r.index.packages.iter().find(|e| e.name == what) {
            anstream::println!("{:>10}  {}", "Name".bold(), e.name);
            anstream::println!("{:>10}  {}", "Version".bold(), e.pkg_version());
            anstream::println!("{:>10}  {}", "Summary".bold(), e.summary);
            anstream::println!("{:>10}  {}", "Repo".bold(), r.location);
            return Ok(());
        }
    }
    bail!("{what} is not installed or in any given repository")
}

fn files(root: &Path, name: &str) -> Result<()> {
    let db = open_db(root)?;
    let i = db
        .get(name)?
        .ok_or_else(|| anyhow!("{name} is not installed"))?;
    for f in &i.manifest.files {
        match f.kind {
            FileKind::Dir => anstream::println!("/{}/", f.path.blue()),
            FileKind::File => anstream::println!("/{}", f.path),
            FileKind::Symlink => anstream::println!(
                "/{} -> {}",
                f.path.cyan(),
                f.target.as_deref().unwrap_or("")
            ),
        }
    }
    Ok(())
}

fn owns(root: &Path, path: &str) -> Result<()> {
    let db = open_db(root)?;
    // Accept /usr/bin/x, usr/bin/x, or <root>/usr/bin/x.
    let p = Path::new(path);
    let rel = p
        .strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_string();
    let owners = db.owners(&rel)?;
    if owners.is_empty() {
        bail!("/{rel} is not owned by any package");
    }
    anstream::println!("/{rel} is owned by {}", owners.join(", ").bold());
    Ok(())
}

fn list(root: &Path) -> Result<()> {
    let db = open_db(root)?;
    for i in db.installed()? {
        let tag = if i.explicit { "" } else { " (dependency)" };
        anstream::println!(
            "{} {}{}",
            i.manifest.name.bold(),
            i.manifest.pkg_version(),
            tag.dimmed()
        );
    }
    Ok(())
}

fn history(root: &Path) -> Result<()> {
    let db = open_db(root)?;
    for h in db.history()? {
        let change = match (&h.old_version, &h.new_version) {
            (None, Some(n)) => n.clone(),
            (Some(o), Some(n)) => format!("{o} -> {n}"),
            (Some(o), None) => o.clone(),
            (None, None) => String::new(),
        };
        let action = match h.action.as_str() {
            "install" => h.action.green().to_string(),
            "remove" => h.action.red().to_string(),
            _ => h.action.yellow().to_string(),
        };
        anstream::println!(
            "{:>5}  {:>10}  {action:<8} {} {change}",
            h.id,
            h.ts,
            h.package.bold()
        );
    }
    Ok(())
}

fn verify(root: &Path) -> Result<()> {
    let db = open_db(root)?;
    let mut bad = 0;
    for i in db.installed()? {
        for f in &i.manifest.files {
            if tx::is_modified(root, f) {
                bad += 1;
                anstream::println!("{} /{} ({})", "modified".red(), f.path, i.manifest.name);
            }
        }
    }
    if bad > 0 {
        bail!("{bad} file(s) differ from the package database");
    }
    anstream::println!("{} every installed file matches", "✓".green());
    Ok(())
}

fn load_key(path: &Path) -> Result<SecretKey> {
    SecretKey::load(path).with_context(|| {
        format!(
            "loading signing key {} (create one with `spk key gen dev --dir {}`)",
            path.display(),
            path.parent()
                .map_or(".".into(), |p| p.display().to_string())
        )
    })
}

fn base_config(o: &BuildOpts, repo: PathBuf) -> Result<BuildConfig> {
    Ok(BuildConfig {
        work: o.work.clone(),
        sources: o.sources.clone(),
        repo,
        repo_name: "solace-local".into(),
        key: load_key(&o.key)?,
        trusted: Vec::new(),
        host: HostTools::detect().ok(),
        default_mode: ToolchainMode::Native,
        mode_override: None,
        dep_repos: Vec::new(),
        base_deps: Vec::new(),
        target: build::TARGET.to_string(),
        keep_work: o.keep_work,
        jobs: o
            .jobs
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from)),
        build_uid: build::DEFAULT_BUILD_UID,
        force: o.force,
    })
}

fn build_cmd(a: BuildArgs) -> Result<()> {
    let all = build::load_recipes(&a.recipes_dir)?;
    let wanted: Vec<String> = if a.all {
        all.iter().map(|r| r.name.clone()).collect()
    } else if a.recipes.is_empty() {
        bail!("name recipes to build, or pass --all");
    } else {
        a.recipes
            .iter()
            .map(|r| {
                Path::new(r)
                    .file_name()
                    .map_or_else(|| r.clone(), |n| n.to_string_lossy().into_owned())
            })
            .collect()
    };
    let order = build::build_order(&all, &wanted)?;
    let mut cfg = base_config(&a.opts, a.repo_dir)?;
    cfg.mode_override = a.toolchain;
    cfg.dep_repos = a.dep_repos;
    cfg.base_deps = a
        .base
        .iter()
        .map(|b| DepSpec::parse(b))
        .collect::<libspk::Result<_>>()?;
    run_builds(&order, &cfg, "")
}

fn bootstrap_cmd(a: &BootstrapArgs) -> Result<()> {
    let plan = Plan::load(&a.plan, Path::new("."))?;
    for st in &plan.stages {
        if !a.stages.is_empty() && !a.stages.contains(&st.name) {
            continue;
        }
        anstream::println!(
            "\n{} {} ({:?}): {}",
            "Stage".cyan().bold(),
            st.name.bold(),
            st.toolchain,
            st.description
        );
        let mut recipes = Vec::new();
        for name in &st.recipes {
            recipes.push(libspk::recipe::Recipe::load(&st.recipes_dir.join(name))?);
            if a.until.as_deref() == Some(name) {
                break;
            }
        }
        let mut cfg = base_config(&a.opts, st.repo.clone())?;
        cfg.default_mode = st.toolchain;
        cfg.dep_repos.clone_from(&st.dep_repos);
        cfg.base_deps.clone_from(&st.base);
        cfg.target.clone_from(&plan.target);
        // Each stage keeps its own build stamps.
        cfg.work = a.opts.work.join(&st.name);
        let order: Vec<&libspk::recipe::Recipe> = recipes.iter().collect();
        run_builds(&order, &cfg, &format!("{} ", st.name))?;
        if a.until.as_ref().is_some_and(|u| st.recipes.contains(u)) {
            break;
        }
    }
    Ok(())
}

fn run_builds(order: &[&libspk::recipe::Recipe], cfg: &BuildConfig, label: &str) -> Result<()> {
    let total = order.len();
    for (n, r) in order.iter().enumerate() {
        let started = std::time::Instant::now();
        let pb = spinner(&format!("{label}[{}/{total}]", n + 1));
        pb.set_message(format!("{} {}", r.name, r.pkg_version()));
        let res = build::build(r, cfg, &mut |ev| match ev {
            BuildEvent::Fetching { url } => pb.set_message(format!("{}: fetching {url}", r.name)),
            BuildEvent::InstallingDeps { count } => {
                pb.set_message(format!("{}: installing {count} build dependencies", r.name));
            }
            BuildEvent::Phase { phase, .. } => pb.set_message(format!("{}: {phase}", r.name)),
            BuildEvent::Packaged { .. } | BuildEvent::Skipped { .. } => {}
        });
        pb.finish_and_clear();
        let built = res.with_context(|| format!("building {}", r.name))?;
        if built.skipped {
            anstream::println!(
                "{} {} {} (unchanged)",
                "=".dimmed(),
                r.name,
                r.pkg_version()
            );
        } else {
            let names: Vec<String> = built
                .packages
                .iter()
                .filter_map(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
                .collect();
            anstream::println!(
                "{} {} {} in {:.0?}: {}",
                "✓".green(),
                r.name.bold(),
                r.pkg_version(),
                started.elapsed(),
                names.join(", ")
            );
        }
    }
    Ok(())
}
