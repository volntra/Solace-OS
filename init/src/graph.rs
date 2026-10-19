//! Start ordering: which units to start, and in what order.

use std::collections::{BTreeMap, BTreeSet};

use crate::unit::Unit;
use crate::{Error, Result};

/// The enabled units plus everything they (transitively) require.
pub fn closure(units: &BTreeMap<String, Unit>, roots: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut todo: Vec<&str> = roots.iter().map(String::as_str).collect();
    while let Some(n) = todo.pop() {
        if !out.insert(n.to_string()) {
            continue;
        }
        if let Some(u) = units.get(n) {
            todo.extend(u.requires.iter().map(String::as_str));
        }
    }
    out
}

/// Edges for a unit: hard requirements plus `after` entries that are also
/// being started.
pub fn waits_for<'a>(u: &'a Unit, set: &BTreeSet<String>) -> Vec<&'a str> {
    u.requires
        .iter()
        .chain(u.after.iter().filter(|a| set.contains(*a)))
        .map(String::as_str)
        .collect()
}

/// A start order for `set` (each unit after everything it waits for),
/// grouped into layers that may start in parallel.
///
/// # Errors
/// [`Error::Unit`] naming the units in a dependency cycle.
pub fn layers(units: &BTreeMap<String, Unit>, set: &BTreeSet<String>) -> Result<Vec<Vec<String>>> {
    let mut remaining: BTreeMap<&str, BTreeSet<&str>> = set
        .iter()
        .filter_map(|n| units.get(n))
        .map(|u| (u.name.as_str(), waits_for(u, set).into_iter().collect()))
        .collect();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let mut out = Vec::new();
    while !remaining.is_empty() {
        let ready: Vec<&str> = remaining
            .iter()
            .filter(|(_, deps)| deps.iter().all(|d| done.contains(d)))
            .map(|(n, _)| *n)
            .collect();
        if ready.is_empty() {
            let stuck: Vec<&str> = remaining.keys().copied().collect();
            return Err(Error::Unit(format!(
                "dependency cycle among: {}",
                stuck.join(", ")
            )));
        }
        for n in &ready {
            remaining.remove(n);
            done.insert(n);
        }
        out.push(ready.into_iter().map(String::from).collect());
    }
    Ok(out)
}

/// Shutdown order: the reverse of the start order.
///
/// # Errors
/// As [`layers`].
pub fn stop_order(units: &BTreeMap<String, Unit>, set: &BTreeSet<String>) -> Result<Vec<String>> {
    Ok(layers(units, set)?.into_iter().flatten().rev().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unit::tests::unit;

    fn table(us: Vec<crate::unit::Unit>) -> BTreeMap<String, Unit> {
        us.into_iter().map(|u| (u.name.clone(), u)).collect()
    }

    fn set(xs: &[&str]) -> BTreeSet<String> {
        xs.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn closure_pulls_requirements() {
        let t = table(vec![
            unit("getty", &["early"]),
            unit("early", &["mounts"]),
            unit("mounts", &[]),
            unit("other", &[]),
        ]);
        assert_eq!(
            closure(&t, &set(&["getty"])),
            set(&["early", "getty", "mounts"])
        );
    }

    #[test]
    fn layering_and_parallelism() {
        let t = table(vec![
            unit("mounts", &[]),
            unit("early", &["mounts"]),
            unit("getty1", &["early"]),
            unit("getty2", &["early"]),
            unit("log", &[]),
        ]);
        let all = set(&["mounts", "early", "getty1", "getty2", "log"]);
        let l = layers(&t, &all).unwrap();
        assert_eq!(
            l,
            vec![
                vec!["log", "mounts"],
                vec!["early"],
                vec!["getty1", "getty2"]
            ]
        );
        let stop = stop_order(&t, &all).unwrap();
        assert_eq!(stop.first().unwrap(), "getty2");
        assert_eq!(stop.last().unwrap(), "log");
        let pos = |n: &str| stop.iter().position(|x| x == n).unwrap();
        assert!(pos("getty1") < pos("early") && pos("early") < pos("mounts"));
    }

    #[test]
    fn after_only_orders_when_both_start() {
        let mut b = unit("b", &[]);
        b.after = vec!["a".into()];
        let t = table(vec![unit("a", &[]), b]);
        assert_eq!(
            layers(&t, &set(&["a", "b"])).unwrap(),
            vec![vec!["a"], vec!["b"]]
        );
        assert_eq!(layers(&t, &set(&["b"])).unwrap(), vec![vec!["b"]]);
    }

    #[test]
    fn cycles_are_reported() {
        let t = table(vec![
            unit("a", &["b"]),
            unit("b", &["c"]),
            unit("c", &["a"]),
            unit("d", &[]),
        ]);
        let e = layers(&t, &set(&["a", "b", "c", "d"]))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("cycle") && e.contains("a, b, c") && !e.contains(", d"),
            "{e}"
        );
    }
}
