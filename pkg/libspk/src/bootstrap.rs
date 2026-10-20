//! Bootstrap plans: ordered build stages (docs/formats/bootstrap-plan.md).
//!
//! A plan says, stage by stage, which recipes to build, in what order,
//! with which toolchain mode, and which repositories build deps may come
//! from. `spk bootstrap <plan>` runs it; every step is an ordinary
//! `spk build`, so every file still comes from a signed package.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::build::{TARGET, ToolchainMode};
use crate::error::{Error, IoContext, Result};
use crate::version::{DepSpec, validate_name};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Target triple (`$FERN_TGT`).
    #[serde(default = "default_target")]
    pub target: String,
    #[serde(rename = "stage")]
    pub stages: Vec<Stage>,
}

fn default_target() -> String {
    TARGET.to_string()
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub name: String,
    pub description: String,
    /// Directory holding this stage's recipes (relative to the plan file's
    /// repository root).
    pub recipes_dir: PathBuf,
    /// Repository the stage's packages go into.
    pub repo: PathBuf,
    /// Further repositories build deps may come from.
    #[serde(default)]
    pub dep_repos: Vec<PathBuf>,
    /// Default toolchain mode for the stage's recipes.
    pub toolchain: ToolchainMode,
    /// Installed into every sysroot of this stage (e.g. the toolchain).
    #[serde(default)]
    pub base: Vec<DepSpec>,
    /// Recipe names, built in this order.
    pub recipes: Vec<String>,
}

impl Plan {
    /// Loads and checks a plan. Paths in it are relative to `root`.
    ///
    /// # Errors
    /// I/O, TOML, and validation errors (unknown recipes, duplicates).
    pub fn load(path: &Path, root: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).at(path)?;
        let mut plan: Plan = toml::from_str(&text)
            .map_err(|e| Error::invalid(format!("{}: {e}", path.display())))?;
        if plan.stages.is_empty() {
            return Err(Error::invalid("a bootstrap plan needs at least one stage"));
        }
        let mut names = std::collections::HashSet::new();
        for st in &mut plan.stages {
            if !names.insert(st.name.clone()) {
                return Err(Error::invalid(format!("stage {} appears twice", st.name)));
            }
            st.recipes_dir = root.join(&st.recipes_dir);
            st.repo = root.join(&st.repo);
            for r in &mut st.dep_repos {
                *r = root.join(&*r);
            }
            let mut seen = std::collections::HashSet::new();
            for r in &st.recipes {
                validate_name(r)?;
                if !seen.insert(r) {
                    return Err(Error::invalid(format!(
                        "stage {}: recipe {r} listed twice",
                        st.name
                    )));
                }
                if !st.recipes_dir.join(r).join("recipe.toml").is_file() {
                    return Err(Error::invalid(format!(
                        "stage {}: no recipe {}/{r}/recipe.toml",
                        st.name,
                        st.recipes_dir.display()
                    )));
                }
            }
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(plan: &str) -> (tempfile::TempDir, Result<Plan>) {
        let d = tempfile::tempdir().unwrap();
        for r in ["a", "b"] {
            fs::create_dir_all(d.path().join("rec").join(r)).unwrap();
            fs::write(d.path().join("rec").join(r).join("recipe.toml"), "").unwrap();
        }
        fs::write(d.path().join("plan.toml"), plan).unwrap();
        let p = Plan::load(&d.path().join("plan.toml"), d.path());
        (d, p)
    }

    const GOOD: &str = r#"
[[stage]]
name = "cross"
description = "cross toolchain"
recipes_dir = "rec"
repo = "out/bs"
toolchain = "cross"
recipes = ["a", "b"]

[[stage]]
name = "final"
description = "the system"
recipes_dir = "rec"
repo = "out/final"
dep_repos = ["out/bs"]
toolchain = "native"
base = ["a"]
recipes = ["b"]
"#;

    #[test]
    fn loads_and_resolves_paths() {
        let (d, p) = setup(GOOD);
        let p = p.unwrap();
        assert_eq!(p.target, TARGET);
        assert_eq!(p.stages.len(), 2);
        assert_eq!(p.stages[1].dep_repos, [d.path().join("out/bs")]);
        assert_eq!(p.stages[1].toolchain, ToolchainMode::Native);
    }

    #[test]
    fn rejects_bad_plans() {
        for (text, want) in [
            (
                GOOD.replace("[\"a\", \"b\"]", "[\"a\", \"zzz\"]"),
                "no recipe",
            ),
            (GOOD.replace("[\"a\", \"b\"]", "[\"a\", \"a\"]"), "twice"),
            (
                GOOD.replace("name = \"final\"", "name = \"cross\""),
                "twice",
            ),
            (
                GOOD.replace("toolchain = \"cross\"", "toolchain = \"magic\""),
                "unknown variant",
            ),
        ] {
            let (_d, p) = setup(&text);
            let e = p.unwrap_err().to_string();
            assert!(e.contains(want), "{want}: {e}");
        }
    }
}
