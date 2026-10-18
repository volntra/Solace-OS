//! Dependency resolution with resolvo (SAT-based).
//!
//! The universe is every available candidate (from repos) plus every
//! installed package. A [`Request`] becomes resolvo requirements:
//!
//! * every installed package stays installed (by name) unless it is being
//!   removed or is replaced by a package in the universe that `replaces` it;
//! * every requested spec must be satisfied;
//! * installed versions are *favored* unless upgrading, so `install` does not
//!   upgrade unrelated packages.
//!
//! `provides` are modelled as virtual solvables named after the provided
//! name, each depending on exactly its provider. `conflicts` and `replaces`
//! become resolvo constraints ("if X is installed it must not match ...").

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use resolvo::utils::{Pool, VersionSet};
use resolvo::{
    Candidates, Condition, ConditionId, ConditionalRequirement, Dependencies, DependencyProvider,
    Interner, KnownDependencies, NameId, Problem, SolvableId, Solver, SolverCache, StringId,
    UnsolvableOrCancelled, VersionSetId, VersionSetUnionId,
};

use crate::error::{Error, Result};
use crate::version::{Constraint, DepSpec, Op, PkgVersion};

/// A package the solver may choose.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub name: String,
    pub version: PkgVersion,
    pub depends: Vec<DepSpec>,
    pub provides: Vec<DepSpec>,
    pub conflicts: Vec<DepSpec>,
    pub replaces: Vec<DepSpec>,
    pub installed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub install: Vec<DepSpec>,
    pub remove: Vec<String>,
    /// Prefer the newest candidate of everything, not the installed one.
    pub upgrade: bool,
}

/// What the solver decided, as indexes into the candidate list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// New packages and new versions of installed packages.
    pub install: Vec<usize>,
    /// Installed packages that are removed (names).
    pub remove: Vec<String>,
    /// Installed packages that stay as they are.
    pub keep: Vec<usize>,
}

/// A solvable: a real candidate or a virtual `provides` entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Rec {
    Real(usize),
    Virtual {
        provider: usize,
        version: Option<PkgVersion>,
    },
}

impl fmt::Display for Rec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Real records are displayed via the provider (needs the table).
            Rec::Real(i) => write!(f, "#{i}"),
            Rec::Virtual { provider, .. } => write!(f, "virtual#{provider}"),
        }
    }
}

/// A version set: a constraint, optionally negated (for conflicts).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Vs {
    constraint: Option<Constraint>,
    negate: bool,
}

impl VersionSet for Vs {
    type V = Rec;
}

impl fmt::Display for Vs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self
            .constraint
            .as_ref()
            .map_or_else(|| "*".to_string(), ToString::to_string);
        if self.negate {
            write!(f, "not {inner}")
        } else {
            f.write_str(&inner)
        }
    }
}

struct Provider<'a> {
    cands: &'a [Candidate],
    pool: Pool<Vs, String>,
    by_name: HashMap<NameId, Vec<SolvableId>>,
    favor_installed: bool,
}

impl<'a> Provider<'a> {
    fn new(cands: &'a [Candidate], favor_installed: bool) -> Self {
        let pool: Pool<Vs, String> = Pool::default();
        let mut by_name: HashMap<NameId, Vec<SolvableId>> = HashMap::new();
        for (i, c) in cands.iter().enumerate() {
            let n = pool.intern_package_name(c.name.as_str());
            let s = pool.intern_solvable(n, Rec::Real(i));
            by_name.entry(n).or_default().push(s);
        }
        for (i, c) in cands.iter().enumerate() {
            for p in &c.provides {
                if p.name == c.name {
                    continue;
                }
                let version = p.constraint.as_ref().and_then(|k| {
                    (k.op == Op::Eq)
                        .then(|| PkgVersion::new(k.version.clone(), k.release.unwrap_or(0)))
                });
                let n = pool.intern_package_name(p.name.as_str());
                let s = pool.intern_solvable(
                    n,
                    Rec::Virtual {
                        provider: i,
                        version,
                    },
                );
                by_name.entry(n).or_default().push(s);
            }
        }
        Provider {
            cands,
            pool,
            by_name,
            favor_installed,
        }
    }

    fn rec(&self, s: SolvableId) -> &Rec {
        &self.pool.resolve_solvable(s).record
    }

    fn vs(&self, spec: &DepSpec, negate: bool) -> VersionSetId {
        let n = self.pool.intern_package_name(spec.name.as_str());
        self.pool.intern_version_set(
            n,
            Vs {
                constraint: spec.constraint.clone(),
                negate,
            },
        )
    }

    fn contains(&self, vs: &Vs, rec: &Rec) -> bool {
        let hit = match (&vs.constraint, rec) {
            (None, _) => true,
            (Some(c), Rec::Real(i)) => c.matches(&self.cands[*i].version),
            // An unversioned provide satisfies any constraint.
            (Some(_), Rec::Virtual { version: None, .. }) => true,
            (
                Some(c),
                Rec::Virtual {
                    version: Some(v), ..
                },
            ) => c.matches(v),
        };
        hit != vs.negate
    }

    fn sort_key(&self, s: SolvableId) -> (u8, u8, Option<&PkgVersion>) {
        match self.rec(s) {
            Rec::Real(i) => {
                let c = &self.cands[*i];
                let favored = u8::from(!(self.favor_installed && c.installed));
                (0, favored, Some(&c.version))
            }
            Rec::Virtual { provider, .. } => (1, 0, Some(&self.cands[*provider].version)),
        }
    }
}

impl Interner for Provider<'_> {
    type NameId = NameId;
    type SolvableId = SolvableId;

    fn display_solvable(&self, s: SolvableId) -> impl fmt::Display + '_ {
        match self.rec(s) {
            Rec::Real(i) => format!("{} {}", self.cands[*i].name, self.cands[*i].version),
            Rec::Virtual { provider, .. } => {
                let p = &self.cands[*provider];
                format!(
                    "{} (provided by {} {})",
                    self.pool
                        .resolve_package_name(self.pool.resolve_solvable(s).name),
                    p.name,
                    p.version
                )
            }
        }
    }

    fn display_merged_solvables(&self, solvables: &[SolvableId]) -> impl fmt::Display + '_ {
        solvables
            .iter()
            .map(|s| self.display_solvable(*s).to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn display_name(&self, name: NameId) -> impl fmt::Display + '_ {
        self.pool.resolve_package_name(name).clone()
    }

    fn display_version_set(&self, vs: VersionSetId) -> impl fmt::Display + '_ {
        self.pool.resolve_version_set(vs).to_string()
    }

    fn display_string(&self, id: StringId) -> impl fmt::Display + '_ {
        self.pool.resolve_string(id).to_owned()
    }

    fn version_set_name(&self, vs: VersionSetId) -> NameId {
        self.pool.resolve_version_set_package_name(vs)
    }

    fn solvable_name(&self, s: SolvableId) -> NameId {
        self.pool.resolve_solvable(s).name
    }

    fn version_sets_in_union(&self, u: VersionSetUnionId) -> impl Iterator<Item = VersionSetId> {
        self.pool.resolve_version_set_union(u)
    }

    fn resolve_condition(&self, c: ConditionId) -> Condition {
        self.pool.resolve_condition(c).clone()
    }
}

// resolvo's trait is async; this provider has everything in memory.
#[allow(unknown_lints, clippy::unused_async_trait_impl)]
impl DependencyProvider for Provider<'_> {
    async fn filter_candidates(
        &self,
        candidates: &[SolvableId],
        vs: VersionSetId,
        inverse: bool,
    ) -> Vec<SolvableId> {
        let set = self.pool.resolve_version_set(vs);
        candidates
            .iter()
            .copied()
            .filter(|s| self.contains(set, self.rec(*s)) != inverse)
            .collect()
    }

    async fn get_candidates(&self, name: NameId) -> Option<Candidates> {
        let list = self.by_name.get(&name)?;
        let favored = if self.favor_installed {
            list.iter()
                .copied()
                .find(|s| matches!(self.rec(*s), Rec::Real(i) if self.cands[*i].installed))
        } else {
            None
        };
        Some(Candidates {
            candidates: list.clone(),
            favored,
            ..Candidates::default()
        })
    }

    async fn sort_candidates(&self, _: &SolverCache<Self>, solvables: &mut [SolvableId]) {
        // Real before virtual; favored installed first; then newest first.
        solvables.sort_by(|a, b| {
            let (ka, fa, va) = self.sort_key(*a);
            let (kb, fb, vb) = self.sort_key(*b);
            ka.cmp(&kb).then(fa.cmp(&fb)).then_with(|| vb.cmp(&va))
        });
    }

    async fn get_dependencies(&self, s: SolvableId) -> Dependencies {
        let mut deps = KnownDependencies::default();
        match self.rec(s) {
            Rec::Real(i) => {
                let c = &self.cands[*i];
                for d in &c.depends {
                    deps.requirements
                        .push(ConditionalRequirement::from(self.vs(d, false)));
                }
                for d in c.conflicts.iter().chain(&c.replaces) {
                    if d.name != c.name {
                        deps.constrains.push(self.vs(d, true));
                    }
                }
            }
            Rec::Virtual { provider, .. } => {
                let p = &self.cands[*provider];
                let exact = DepSpec {
                    name: p.name.clone(),
                    constraint: Some(Constraint {
                        op: Op::Eq,
                        version: p.version.version.clone(),
                        release: Some(p.version.release),
                    }),
                };
                deps.requirements
                    .push(ConditionalRequirement::from(self.vs(&exact, false)));
            }
        }
        Dependencies::Known(deps)
    }
}

/// Resolves `req` over `cands` (repo candidates plus installed packages).
///
/// # Errors
/// [`Error::Solve`] with resolvo's explanation if no solution exists, and
/// [`Error::Conflict`] if a removal would break an installed package.
pub fn solve(cands: &[Candidate], req: &Request) -> Result<Plan> {
    let provider = Provider::new(cands, !req.upgrade);

    let removing: HashSet<&str> = req.remove.iter().map(String::as_str).collect();
    for r in &removing {
        if !cands.iter().any(|c| c.installed && c.name == *r) {
            return Err(Error::Solve(format!("{r} is not installed")));
        }
    }

    // Packages replaced by something requested (install) or by anything
    // available (upgrade) no longer need to stay.
    let replacers: Vec<&Candidate> = cands
        .iter()
        .filter(|c| !c.installed)
        .filter(|c| req.upgrade || req.install.iter().any(|d| d.name == c.name))
        .collect();
    let replaced = |c: &Candidate| {
        replacers
            .iter()
            .any(|r| r.replaces.iter().any(|d| d.matches(&c.name, &c.version)))
    };

    let mut requirements = Vec::new();
    let mut constraints = Vec::new();
    let mut kept_names = BTreeSet::new();
    for c in cands.iter().filter(|c| c.installed) {
        if removing.contains(c.name.as_str()) {
            // Forbid it outright so dependents surface as a conflict.
            constraints.push(provider.vs(
                &DepSpec {
                    name: c.name.clone(),
                    constraint: None,
                },
                true,
            ));
        } else if !replaced(c) {
            kept_names.insert(c.name.clone());
        }
    }
    for name in &kept_names {
        let spec = DepSpec {
            name: name.clone(),
            constraint: None,
        };
        requirements.push(ConditionalRequirement::from(provider.vs(&spec, false)));
    }
    for d in &req.install {
        requirements.push(ConditionalRequirement::from(provider.vs(d, false)));
    }

    let mut solver = Solver::new(provider);
    let problem = Problem::new()
        .requirements(requirements)
        .constraints(constraints);
    let solution = match solver.solve(problem) {
        Ok(s) => s,
        Err(UnsolvableOrCancelled::Unsolvable(conflict)) => {
            let why = conflict.display_user_friendly(&solver).to_string();
            if !removing.is_empty() {
                let mut needed_by = Vec::new();
                for c in cands.iter().filter(|c| c.installed) {
                    if c.depends.iter().any(|d| removing.contains(d.name.as_str()))
                        && !removing.contains(c.name.as_str())
                    {
                        needed_by.push(c.name.clone());
                    }
                }
                if !needed_by.is_empty() {
                    return Err(Error::Conflict(format!(
                        "cannot remove {}: required by {}",
                        req.remove.join(", "),
                        needed_by.join(", ")
                    )));
                }
            }
            return Err(Error::Solve(format!(
                "no set of packages satisfies the request:\n{why}"
            )));
        }
        Err(UnsolvableOrCancelled::Cancelled(_)) => {
            return Err(Error::Solve("dependency resolution was cancelled".into()));
        }
    };

    let provider = solver.provider();
    let chosen: HashSet<usize> = solution
        .iter()
        .filter_map(|s| match provider.rec(*s) {
            Rec::Real(i) => Some(*i),
            Rec::Virtual { .. } => None,
        })
        .collect();
    Ok(build_plan(cands, &chosen, &removing))
}

/// Turns the chosen candidates into install / keep / remove lists.
fn build_plan(cands: &[Candidate], chosen: &HashSet<usize>, removing: &HashSet<&str>) -> Plan {
    let mut plan = Plan::default();
    let mut chosen_sorted: Vec<usize> = chosen.iter().copied().collect();
    chosen_sorted.sort_unstable();
    for i in chosen_sorted {
        if cands[i].installed {
            plan.keep.push(i);
        } else {
            plan.install.push(i);
        }
    }
    let chosen_names: HashSet<&str> = chosen.iter().map(|i| cands[*i].name.as_str()).collect();
    for c in cands.iter().filter(|c| c.installed) {
        let still_there = chosen
            .iter()
            .any(|i| cands[*i].installed && cands[*i].name == c.name);
        // Not chosen at all (removed or replaced), as opposed to upgraded,
        // where a new version of the same name is in `install`.
        if !still_there
            && (!chosen_names.contains(c.name.as_str()) || removing.contains(c.name.as_str()))
            && !plan.remove.contains(&c.name)
        {
            plan.remove.push(c.name.clone());
        }
    }
    plan.remove.sort();
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::Version;

    fn cand(name: &str, ver: &str, deps: &[&str], installed: bool) -> Candidate {
        let (v, r) = ver.split_once('-').unwrap_or((ver, "1"));
        Candidate {
            name: name.into(),
            version: PkgVersion::new(Version::parse(v).unwrap(), r.parse().unwrap()),
            depends: deps.iter().map(|d| DepSpec::parse(d).unwrap()).collect(),
            provides: vec![],
            conflicts: vec![],
            replaces: vec![],
            installed,
        }
    }

    fn install(names: &[&str]) -> Request {
        Request {
            install: names.iter().map(|d| DepSpec::parse(d).unwrap()).collect(),
            ..Request::default()
        }
    }

    fn names(c: &[Candidate], idx: &[usize]) -> Vec<String> {
        let mut v: Vec<String> = idx
            .iter()
            .map(|i| format!("{}-{}", c[*i].name, c[*i].version))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn pulls_transitive_deps_newest_first() {
        let c = vec![
            cand("libarchive", "3.8.1", &["zlib", "xz>=5.6"], false),
            cand("zlib", "1.3", &[], false),
            cand("zlib", "1.3.1", &[], false),
            cand("xz", "5.4.0", &[], false),
            cand("xz", "5.8.1", &["zlib"], false),
            cand("unrelated", "1", &[], false),
        ];
        let p = solve(&c, &install(&["libarchive"])).unwrap();
        assert_eq!(
            names(&c, &p.install),
            ["libarchive-3.8.1-1", "xz-5.8.1-1", "zlib-1.3.1-1"]
        );
        assert!(p.remove.is_empty());
    }

    #[test]
    fn version_constraint_selects_older() {
        let c = vec![
            cand("app", "1", &["zlib<1.3.1"], false),
            cand("zlib", "1.3", &[], false),
            cand("zlib", "1.3.1", &[], false),
        ];
        let p = solve(&c, &install(&["app"])).unwrap();
        assert_eq!(names(&c, &p.install), ["app-1-1", "zlib-1.3-1"]);
    }

    #[test]
    fn install_keeps_installed_versions() {
        let c = vec![
            cand("zlib", "1.3", &[], true),
            cand("zlib", "1.3.1", &[], false),
            cand("app", "1", &["zlib"], false),
        ];
        let p = solve(&c, &install(&["app"])).unwrap();
        assert_eq!(names(&c, &p.install), ["app-1-1"]);
        assert_eq!(names(&c, &p.keep), ["zlib-1.3-1"]);
    }

    #[test]
    fn upgrade_moves_to_newest() {
        let c = vec![
            cand("zlib", "1.3", &[], true),
            cand("zlib", "1.3.1", &[], false),
        ];
        let p = solve(
            &c,
            &Request {
                upgrade: true,
                ..Request::default()
            },
        )
        .unwrap();
        assert_eq!(names(&c, &p.install), ["zlib-1.3.1-1"]);
        assert!(
            p.remove.is_empty(),
            "upgrade is not a removal: {:?}",
            p.remove
        );
    }

    #[test]
    fn unsatisfiable_explains() {
        let c = vec![
            cand("app", "1", &["zlib>=2"], false),
            cand("zlib", "1.3", &[], false),
        ];
        let e = solve(&c, &install(&["app"])).unwrap_err().to_string();
        assert!(e.contains("zlib"), "{e}");
        let e = solve(&c, &install(&["nope"])).unwrap_err().to_string();
        assert!(e.contains("nope"), "{e}");
    }

    #[test]
    fn remove_blocked_by_dependent() {
        let c = vec![
            cand("zlib", "1.3", &[], true),
            cand("app", "1", &["zlib"], true),
        ];
        let req = Request {
            remove: vec!["zlib".into()],
            ..Request::default()
        };
        let e = solve(&c, &req).unwrap_err().to_string();
        assert!(e.contains("required by app"), "{e}");

        let req = Request {
            remove: vec!["zlib".into(), "app".into()],
            ..Request::default()
        };
        let p = solve(&c, &req).unwrap();
        assert_eq!(p.remove, ["app", "zlib"]);
    }

    #[test]
    fn remove_not_installed() {
        let c = vec![cand("zlib", "1.3", &[], false)];
        let req = Request {
            remove: vec!["zlib".into()],
            ..Request::default()
        };
        assert!(solve(&c, &req).is_err());
    }

    #[test]
    fn provides_satisfy_dependencies() {
        let mut provider = cand("libxcrypt", "4.4", &[], false);
        provider.provides = vec![DepSpec::parse("libcrypt=4.4-1").unwrap()];
        let c = vec![provider, cand("pam", "1.6", &["libcrypt>=4"], false)];
        let p = solve(&c, &install(&["pam"])).unwrap();
        assert_eq!(names(&c, &p.install), ["libxcrypt-4.4-1", "pam-1.6-1"]);
    }

    #[test]
    fn conflicts_are_enforced() {
        let mut a = cand("a", "1", &[], false);
        a.conflicts = vec![DepSpec::parse("b").unwrap()];
        let c = vec![a, cand("b", "1", &[], false)];
        assert!(solve(&c, &install(&["a", "b"])).is_err());
        solve(&c, &install(&["a"])).unwrap();

        // A versioned conflict only blocks matching versions.
        let mut a = cand("a", "1", &[], false);
        a.conflicts = vec![DepSpec::parse("b<2").unwrap()];
        let c = vec![a, cand("b", "1", &[], false), cand("b", "2", &[], false)];
        let p = solve(&c, &install(&["a", "b"])).unwrap();
        assert_eq!(names(&c, &p.install), ["a-1-1", "b-2-1"]);
    }

    #[test]
    fn replaces_removes_the_old_package() {
        let mut new = cand("liblzma", "5.8.1", &[], false);
        new.replaces = vec![DepSpec::parse("xz-libs").unwrap()];
        let c = vec![cand("xz-libs", "5.4", &[], true), new];
        let p = solve(&c, &install(&["liblzma"])).unwrap();
        assert_eq!(names(&c, &p.install), ["liblzma-5.8.1-1"]);
        assert_eq!(p.remove, ["xz-libs"]);
    }

    #[test]
    fn already_installed_is_noop() {
        let c = vec![cand("zlib", "1.3", &[], true)];
        let p = solve(&c, &install(&["zlib"])).unwrap();
        assert!(p.install.is_empty() && p.remove.is_empty());
        assert_eq!(p.keep, [0]);
    }
}
