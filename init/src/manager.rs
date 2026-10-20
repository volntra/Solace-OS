//! The service manager's decisions, free of side effects.
//!
//! [`Manager`] holds every unit's state and turns events (a process was
//! spawned, exited, became ready; a timer fired; fernctl asked for
//! something) into [`Action`]s for the runtime to carry out. Keeping the
//! decisions pure makes ordering, restart, and shutdown behaviour testable
//! without real processes.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::graph;
use crate::policy::{Decision, Exit, History};
use crate::proto::UnitStatus;
use crate::unit::{ServiceType, Unit};
use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not requested.
    Inactive,
    /// Requested; waiting for its requirements.
    Waiting,
    /// Process running but not yet ready (oneshot, notify).
    Starting {
        pid: u32,
    },
    /// Ready (simple: as soon as it runs).
    Running {
        pid: u32,
    },
    /// A oneshot that finished successfully.
    Done,
    /// Being stopped; `restart` says whether to start again afterwards.
    Stopping {
        pid: u32,
        deadline: u64,
        killed: bool,
        restart: bool,
    },
    /// Crashed; starts again at `at`.
    Restarting {
        at: u64,
    },
    /// Stopped on request or after a clean exit.
    Stopped,
    Failed(String),
}

impl State {
    fn pid(&self) -> Option<u32> {
        match self {
            State::Starting { pid } | State::Running { pid } | State::Stopping { pid, .. } => {
                Some(*pid)
            }
            _ => None,
        }
    }

    /// Satisfies units that require this one.
    fn satisfies(&self) -> bool {
        matches!(self, State::Running { .. } | State::Done)
    }

    /// Still on its way up (counts against "boot complete").
    fn pending(&self) -> bool {
        matches!(self, State::Waiting | State::Starting { .. })
    }

    pub fn label(&self) -> String {
        match self {
            State::Inactive => "inactive".into(),
            State::Waiting => "waiting".into(),
            State::Starting { .. } => "starting".into(),
            State::Running { .. } => "running".into(),
            State::Done => "done".into(),
            State::Stopping { .. } => "stopping".into(),
            State::Restarting { .. } => "restarting".into(),
            State::Stopped => "stopped".into(),
            State::Failed(why) => format!("failed: {why}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sig {
    Term,
    Kill,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Spawn(String),
    Signal {
        unit: String,
        pid: u32,
        sig: Sig,
    },
    /// Something worth recording in fern-log.
    Log {
        unit: String,
        msg: String,
    },
}

#[derive(Debug)]
struct Entry {
    unit: Unit,
    state: State,
    since: u64,
    started_at: u64,
    history: History,
}

#[derive(Debug)]
pub struct Manager {
    units: BTreeMap<String, Entry>,
    enabled: BTreeSet<String>,
    boot_set: BTreeSet<String>,
    boot_done_at: Option<u64>,
    /// Remaining shutdown layers (each stops completely before the next).
    shutdown: Option<Vec<Vec<String>>>,
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl Manager {
    /// # Errors
    /// Dependency cycles among the units.
    pub fn new(units: BTreeMap<String, Unit>, enabled: BTreeSet<String>) -> Result<Self> {
        let all: BTreeSet<String> = units.keys().cloned().collect();
        graph::layers(&units, &all)?;
        Ok(Manager {
            units: units
                .into_iter()
                .map(|(n, u)| {
                    (
                        n,
                        Entry {
                            unit: u,
                            state: State::Inactive,
                            since: 0,
                            started_at: 0,
                            history: History::default(),
                        },
                    )
                })
                .collect(),
            enabled,
            boot_set: BTreeSet::new(),
            boot_done_at: None,
            shutdown: None,
        })
    }

    pub fn unit(&self, name: &str) -> Option<&Unit> {
        self.units.get(name).map(|e| &e.unit)
    }

    pub fn state(&self, name: &str) -> Option<&State> {
        self.units.get(name).map(|e| &e.state)
    }

    fn set(&mut self, name: &str, state: State, now: u64) {
        if let Some(e) = self.units.get_mut(name) {
            e.state = state;
            e.since = now;
        }
    }

    fn table(&self) -> BTreeMap<String, Unit> {
        self.units
            .iter()
            .map(|(n, e)| (n.clone(), e.unit.clone()))
            .collect()
    }

    /// Requests the enabled units (and their requirements) at boot.
    pub fn boot(&mut self, now: u64) {
        let set = graph::closure(&self.table(), &self.enabled);
        self.boot_set.clone_from(&set);
        for n in set {
            self.set(&n, State::Waiting, now);
        }
    }

    /// Starts a unit and its requirements.
    ///
    /// # Errors
    /// Unknown unit.
    pub fn start(&mut self, name: &str, now: u64) -> Result<()> {
        if !self.units.contains_key(name) {
            return Err(Error::Unit(format!("no unit named {name}")));
        }
        let mut roots = BTreeSet::new();
        roots.insert(name.to_string());
        for n in graph::closure(&self.table(), &roots) {
            let e = &self.units[&n];
            if matches!(e.state, State::Inactive | State::Stopped | State::Failed(_)) {
                if let Some(e) = self.units.get_mut(&n) {
                    e.history.reset();
                }
                self.set(&n, State::Waiting, now);
            }
        }
        Ok(())
    }

    /// Stops a unit (not its requirements).
    ///
    /// # Errors
    /// Unknown unit.
    pub fn stop(&mut self, name: &str, now: u64) -> Result<Vec<Action>> {
        self.stop_inner(name, false, now)
    }

    /// Stops a unit and starts it again (crash history cleared).
    ///
    /// # Errors
    /// Unknown unit.
    pub fn restart(&mut self, name: &str, now: u64) -> Result<Vec<Action>> {
        if let Some(e) = self.units.get_mut(name) {
            e.history.reset();
        }
        let running = self
            .units
            .get(name)
            .is_some_and(|e| e.state.pid().is_some());
        if running {
            self.stop_inner(name, true, now)
        } else {
            self.start(name, now)?;
            Ok(Vec::new())
        }
    }

    fn stop_inner(&mut self, name: &str, restart: bool, now: u64) -> Result<Vec<Action>> {
        let e = self
            .units
            .get(name)
            .ok_or_else(|| Error::Unit(format!("no unit named {name}")))?;
        let timeout = e.unit.service.stop_timeout_ms;
        match e.state.clone() {
            State::Starting { pid } | State::Running { pid } => {
                self.set(
                    name,
                    State::Stopping {
                        pid,
                        deadline: now + timeout,
                        killed: false,
                        restart,
                    },
                    now,
                );
                Ok(vec![Action::Signal {
                    unit: name.into(),
                    pid,
                    sig: Sig::Term,
                }])
            }
            State::Stopping { .. } => Ok(Vec::new()),
            _ => {
                self.set(
                    name,
                    if restart {
                        State::Waiting
                    } else {
                        State::Stopped
                    },
                    now,
                );
                Ok(Vec::new())
            }
        }
    }

    /// The runtime started `name` as `pid`.
    pub fn spawned(&mut self, name: &str, pid: u32, now: u64) {
        let kind = self.units.get(name).map(|e| e.unit.service.kind);
        let state = match kind {
            Some(ServiceType::Simple) => State::Running { pid },
            _ => State::Starting { pid },
        };
        self.set(name, state, now);
        if let Some(e) = self.units.get_mut(name) {
            e.started_at = now;
        }
    }

    /// The runtime could not start `name`.
    pub fn spawn_failed(&mut self, name: &str, why: &str, now: u64) {
        self.set(name, State::Failed(format!("could not start: {why}")), now);
    }

    /// A notify service reported READY=1.
    pub fn ready(&mut self, name: &str, now: u64) {
        if let Some(State::Starting { pid }) = self.state(name).cloned()
            && self.units[name].unit.service.kind == ServiceType::Notify
        {
            self.set(name, State::Running { pid }, now);
        }
    }

    fn unit_of_pid(&self, pid: u32) -> Option<String> {
        self.units
            .iter()
            .find(|(_, e)| e.state.pid() == Some(pid))
            .map(|(n, _)| n.clone())
    }

    /// A child process ended. Returns log actions; unknown pids (orphans
    /// reparented to PID 1) are ignored.
    pub fn exited(&mut self, pid: u32, exit: Exit, now: u64) -> Vec<Action> {
        let Some(name) = self.unit_of_pid(pid) else {
            return Vec::new();
        };
        let mut out = vec![Action::Log {
            unit: name.clone(),
            msg: format!("process {pid} exited ({})", describe(exit)),
        }];
        let Some(e) = self.units.get_mut(&name) else {
            return out;
        };
        if let State::Stopping { restart, .. } = e.state {
            let next = if restart {
                State::Waiting
            } else {
                State::Stopped
            };
            self.set(&name, next, now);
            return out;
        }
        let svc = &e.unit.service;
        let decision = e.history.on_exit(
            svc.kind,
            svc.restart,
            Duration::from_millis(svc.restart_delay_ms),
            exit,
            e.started_at,
            now,
        );
        let next = match decision {
            Decision::Done => State::Done,
            Decision::Stopped => State::Stopped,
            Decision::RestartAfter(d) => {
                out.push(Action::Log {
                    unit: name.clone(),
                    msg: format!("restarting in {} ms", ms(d)),
                });
                State::Restarting { at: now + ms(d) }
            }
            Decision::Failed => State::Failed(describe(exit)),
        };
        self.set(&name, next, now);
        out
    }

    /// Advances timers and starts whatever is unblocked.
    pub fn tick(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        let names: Vec<String> = self.units.keys().cloned().collect();
        for n in &names {
            match self.units[n].state.clone() {
                State::Restarting { at } if at <= now && self.shutdown.is_none() => {
                    self.set(n, State::Waiting, now);
                }
                State::Stopping {
                    pid,
                    deadline,
                    killed: false,
                    restart,
                } if deadline <= now => {
                    out.push(Action::Log {
                        unit: n.clone(),
                        msg: "did not stop in time; killing".into(),
                    });
                    out.push(Action::Signal {
                        unit: n.clone(),
                        pid,
                        sig: Sig::Kill,
                    });
                    self.set(
                        n,
                        State::Stopping {
                            pid,
                            deadline,
                            killed: true,
                            restart,
                        },
                        now,
                    );
                }
                _ => {}
            }
        }
        if self.shutdown.is_none() {
            out.extend(self.start_unblocked(now));
        } else {
            out.extend(self.advance_shutdown(now));
        }
        if self.boot_done_at.is_none()
            && !self.boot_set.is_empty()
            && self.boot_set.iter().all(|n| !self.units[n].state.pending())
        {
            self.boot_done_at = Some(now);
        }
        out
    }

    fn start_unblocked(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        let waiting: Vec<String> = self
            .units
            .iter()
            .filter(|(_, e)| e.state == State::Waiting)
            .map(|(n, _)| n.clone())
            .collect();
        let active: BTreeSet<String> = self
            .units
            .iter()
            .filter(|(_, e)| !matches!(e.state, State::Inactive | State::Stopped))
            .map(|(n, _)| n.clone())
            .collect();
        for n in waiting {
            let u = &self.units[&n].unit;
            // A failed requirement fails its dependents.
            if let Some(bad) = u.requires.iter().find(|r| {
                matches!(
                    self.units[*r].state,
                    State::Failed(_) | State::Stopped | State::Inactive
                )
            }) {
                let why = format!("requirement {bad} is {}", self.units[bad].state.label());
                out.push(Action::Log {
                    unit: n.clone(),
                    msg: format!("not started: {why}"),
                });
                self.set(&n, State::Failed(why), now);
                continue;
            }
            let blocked = graph::waits_for(u, &active)
                .iter()
                .any(|d| !self.units[*d].state.satisfies());
            if !blocked {
                out.push(Action::Spawn(n.clone()));
                // The runtime reports back with spawned()/spawn_failed().
                self.set(&n, State::Starting { pid: 0 }, now);
            }
        }
        out
    }

    /// Begins an orderly shutdown: units stop in reverse start order, one
    /// layer at a time.
    pub fn begin_shutdown(&mut self, now: u64) -> Vec<Action> {
        if self.shutdown.is_some() {
            return Vec::new();
        }
        let live: BTreeSet<String> = self
            .units
            .iter()
            .filter(|(_, e)| {
                !matches!(e.state, State::Inactive | State::Stopped | State::Failed(_))
            })
            .map(|(n, _)| n.clone())
            .collect();
        for n in &live {
            if matches!(
                self.units[n].state,
                State::Waiting | State::Restarting { .. }
            ) {
                self.set(n, State::Stopped, now);
            }
        }
        let mut layers = graph::layers(&self.table(), &live).unwrap_or_default();
        layers.reverse();
        self.shutdown = Some(layers);
        self.advance_shutdown(now)
    }

    fn advance_shutdown(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        loop {
            let Some(layers) = self.shutdown.as_mut() else {
                return out;
            };
            let Some(current) = layers.first().cloned() else {
                return out;
            };
            let mut busy = false;
            for n in &current {
                match self.units[n].state {
                    State::Starting { .. } | State::Running { .. } => {
                        if let Ok(a) = self.stop_inner(n, false, now) {
                            out.extend(a);
                        }
                        busy = true;
                    }
                    State::Stopping { .. } => busy = true,
                    _ => {}
                }
            }
            if busy {
                return out;
            }
            if let Some(l) = self.shutdown.as_mut() {
                l.remove(0);
            }
        }
    }

    /// True once every unit has stopped after [`Manager::begin_shutdown`].
    pub fn shutdown_complete(&self) -> bool {
        self.shutdown.as_ref().is_some_and(Vec::is_empty)
    }

    /// When every boot unit was up (monotonic ms), if it is.
    pub fn boot_done_at(&self) -> Option<u64> {
        self.boot_done_at
    }

    /// The earliest pending timer, for the event loop's poll timeout.
    pub fn next_deadline(&self) -> Option<u64> {
        self.units
            .values()
            .filter_map(|e| match e.state {
                State::Restarting { at } => Some(at),
                State::Stopping {
                    deadline,
                    killed: false,
                    ..
                } => Some(deadline),
                _ => None,
            })
            .min()
    }

    pub fn status(&self, now: u64) -> Vec<UnitStatus> {
        self.units
            .iter()
            .map(|(n, e)| UnitStatus {
                name: n.clone(),
                description: e.unit.description.clone(),
                state: e.state.label(),
                pid: e.state.pid().filter(|p| *p != 0),
                for_ms: now.saturating_sub(e.since),
                enabled: self.enabled.contains(n),
            })
            .collect()
    }

    pub fn set_enabled(&mut self, name: &str, on: bool) {
        if on {
            self.enabled.insert(name.to_string());
        } else {
            self.enabled.remove(name);
        }
    }

    /// Pids of every unit process, for signalling whole cgroups.
    pub fn pids(&self) -> Vec<(String, u32)> {
        self.units
            .iter()
            .filter_map(|(n, e)| e.state.pid().filter(|p| *p != 0).map(|p| (n.clone(), p)))
            .collect()
    }
}

pub fn describe(exit: Exit) -> String {
    match exit {
        Exit::Code(0) => "exited cleanly".into(),
        Exit::Code(c) => format!("exit status {c}"),
        Exit::Signal(s) => format!("killed by signal {s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unit::Restart;
    use crate::unit::tests::unit;

    fn mgr(units: Vec<Unit>, enabled: &[&str]) -> Manager {
        Manager::new(
            units.into_iter().map(|u| (u.name.clone(), u)).collect(),
            enabled.iter().map(ToString::to_string).collect(),
        )
        .unwrap()
    }

    fn spawns(a: &[Action]) -> Vec<String> {
        a.iter()
            .filter_map(|x| match x {
                Action::Spawn(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    fn oneshot(name: &str, req: &[&str]) -> Unit {
        let mut u = unit(name, req);
        u.service.kind = ServiceType::Oneshot;
        u.service.restart = Restart::No;
        u
    }

    #[test]
    fn boot_starts_in_dependency_order() {
        let mut m = mgr(
            vec![
                oneshot("mounts", &[]),
                unit("getty", &["mounts"]),
                unit("log", &[]),
            ],
            &["getty", "log"],
        );
        m.boot(0);
        assert_eq!(spawns(&m.tick(0)), ["log", "mounts"]);
        m.spawned("log", 10, 1);
        m.spawned("mounts", 11, 1);
        assert!(spawns(&m.tick(2)).is_empty(), "getty waits for the oneshot");
        assert_eq!(m.boot_done_at(), None);
        m.exited(11, Exit::Code(0), 3);
        assert_eq!(m.state("mounts"), Some(&State::Done));
        assert_eq!(spawns(&m.tick(4)), ["getty"]);
        m.spawned("getty", 12, 5);
        m.tick(6);
        assert_eq!(m.boot_done_at(), Some(6));
    }

    #[test]
    fn failed_requirement_fails_dependents() {
        let mut m = mgr(
            vec![oneshot("mounts", &[]), unit("getty", &["mounts"])],
            &["getty"],
        );
        m.boot(0);
        m.tick(0);
        m.spawned("mounts", 5, 0);
        m.exited(5, Exit::Code(1), 1);
        let acts = m.tick(2);
        assert!(spawns(&acts).is_empty());
        assert!(matches!(m.state("getty"), Some(State::Failed(w)) if w.contains("mounts")));
        assert!(
            m.boot_done_at().is_some(),
            "boot completes even with failures"
        );
    }

    #[test]
    fn crashes_restart_with_backoff() {
        let mut u = unit("svc", &[]);
        u.service.restart = Restart::Always;
        u.service.restart_delay_ms = 100;
        let mut m = mgr(vec![u], &["svc"]);
        m.boot(0);
        m.tick(0);
        m.spawned("svc", 7, 0);
        m.exited(7, Exit::Signal(11), 10);
        assert_eq!(m.state("svc"), Some(&State::Restarting { at: 110 }));
        assert_eq!(m.next_deadline(), Some(110));
        assert!(spawns(&m.tick(50)).is_empty());
        assert_eq!(
            spawns(&m.tick(110)),
            ["svc"],
            "starts as soon as the delay ends"
        );
        m.spawned("svc", 8, 111);
        m.exited(8, Exit::Code(1), 120);
        assert_eq!(m.state("svc"), Some(&State::Restarting { at: 320 }));
    }

    #[test]
    fn unknown_pids_are_ignored() {
        let mut m = mgr(vec![unit("a", &[])], &["a"]);
        assert!(m.exited(4242, Exit::Code(0), 0).is_empty());
    }

    #[test]
    fn stop_terms_then_kills() {
        let mut m = mgr(vec![unit("a", &[])], &["a"]);
        m.boot(0);
        m.tick(0);
        m.spawned("a", 9, 0);
        let acts = m.stop("a", 100).unwrap();
        assert_eq!(
            acts,
            [Action::Signal {
                unit: "a".into(),
                pid: 9,
                sig: Sig::Term
            }]
        );
        assert!(
            m.tick(1000)
                .iter()
                .all(|a| !matches!(a, Action::Signal { .. }))
        );
        let acts = m.tick(5100);
        assert!(acts.contains(&Action::Signal {
            unit: "a".into(),
            pid: 9,
            sig: Sig::Kill
        }));
        assert!(
            m.tick(6000)
                .iter()
                .all(|a| !matches!(a, Action::Signal { .. })),
            "kill once"
        );
        m.exited(9, Exit::Signal(9), 6001);
        assert_eq!(m.state("a"), Some(&State::Stopped));
        assert!(
            spawns(&m.tick(7000)).is_empty(),
            "a stopped unit stays stopped"
        );
    }

    #[test]
    fn restart_command() {
        let mut m = mgr(vec![unit("a", &[])], &["a"]);
        m.boot(0);
        m.tick(0);
        m.spawned("a", 9, 0);
        m.restart("a", 10).unwrap();
        m.exited(9, Exit::Signal(15), 20);
        assert_eq!(m.state("a"), Some(&State::Waiting));
        assert_eq!(spawns(&m.tick(21)), ["a"]);
    }

    #[test]
    fn manual_start_pulls_requirements() {
        let mut m = mgr(vec![unit("base", &[]), unit("app", &["base"])], &[]);
        m.boot(0);
        assert!(spawns(&m.tick(0)).is_empty());
        m.start("app", 1).unwrap();
        assert_eq!(spawns(&m.tick(1)), ["base"]);
        assert!(m.start("nope", 1).is_err());
    }

    #[test]
    fn shutdown_reverses_start_order() {
        let mut m = mgr(
            vec![
                unit("base", &[]),
                unit("mid", &["base"]),
                unit("top", &["mid"]),
            ],
            &["top"],
        );
        m.boot(0);
        let mut pid = 100;
        for t in 0..3 {
            for n in spawns(&m.tick(t)) {
                pid += 1;
                m.spawned(&n, pid, t);
            }
        }
        assert_eq!(m.pids().len(), 3);
        let acts = m.begin_shutdown(10);
        assert_eq!(
            acts,
            [Action::Signal {
                unit: "top".into(),
                pid: 103,
                sig: Sig::Term
            }]
        );
        assert!(!m.shutdown_complete());
        m.exited(103, Exit::Signal(15), 11);
        let acts = m.tick(11);
        assert!(acts.contains(&Action::Signal {
            unit: "mid".into(),
            pid: 102,
            sig: Sig::Term
        }));
        m.exited(102, Exit::Code(0), 12);
        m.tick(12);
        m.exited(101, Exit::Code(0), 13);
        m.tick(13);
        assert!(m.shutdown_complete());
        assert!(
            spawns(&m.tick(20)).is_empty(),
            "nothing restarts during shutdown"
        );
    }

    #[test]
    fn cycles_rejected_up_front() {
        let units: BTreeMap<String, Unit> = [unit("a", &["b"]), unit("b", &["a"])]
            .into_iter()
            .map(|u| (u.name.clone(), u))
            .collect();
        assert!(Manager::new(units, BTreeSet::new()).is_err());
    }
}
