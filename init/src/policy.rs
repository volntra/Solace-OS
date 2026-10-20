//! What to do when a service process exits.

use std::time::Duration;

use crate::unit::{Restart, ServiceType};

/// Delays double per consecutive crash up to this.
pub const MAX_DELAY: Duration = Duration::from_secs(30);
/// Staying up this long resets the crash count.
pub const STABLE_AFTER: Duration = Duration::from_secs(30);
/// More crashes than this within [`CRASH_WINDOW`] mark the unit failed.
pub const MAX_CRASHES: usize = 10;
pub const CRASH_WINDOW: Duration = Duration::from_secs(300);

/// How a process ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
}

impl Exit {
    pub fn success(self) -> bool {
        self == Exit::Code(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// A oneshot finished successfully.
    Done,
    /// Leave it stopped (clean exit, restart = no/on-failure).
    Stopped,
    /// Start it again after this delay.
    RestartAfter(Duration),
    /// Too many crashes, or a failed oneshot: give up until asked.
    Failed,
}

/// Crash history of one unit, in milliseconds since an arbitrary epoch
/// (the manager's monotonic clock).
#[derive(Clone, Debug, Default)]
pub struct History {
    crashes: Vec<u64>,
    consecutive: u32,
}

impl History {
    /// Decides what follows an exit at `now_ms` of a process that started
    /// at `started_ms`.
    pub fn on_exit(
        &mut self,
        kind: ServiceType,
        restart: Restart,
        base_delay: Duration,
        exit: Exit,
        started_ms: u64,
        now_ms: u64,
    ) -> Decision {
        if kind == ServiceType::Oneshot {
            return if exit.success() {
                Decision::Done
            } else if restart == Restart::OnFailure {
                self.crash(base_delay, started_ms, now_ms)
            } else {
                Decision::Failed
            };
        }
        let wants_restart = match restart {
            Restart::No => false,
            Restart::OnFailure => !exit.success(),
            Restart::Always => true,
        };
        if !wants_restart {
            return if exit.success() {
                Decision::Stopped
            } else {
                Decision::Failed
            };
        }
        self.crash(base_delay, started_ms, now_ms)
    }

    fn crash(&mut self, base: Duration, started_ms: u64, now_ms: u64) -> Decision {
        let window = u64::try_from(CRASH_WINDOW.as_millis()).unwrap_or(u64::MAX);
        let stable = u64::try_from(STABLE_AFTER.as_millis()).unwrap_or(u64::MAX);
        if now_ms.saturating_sub(started_ms) >= stable {
            self.consecutive = 0;
        }
        self.crashes.retain(|t| now_ms.saturating_sub(*t) < window);
        self.crashes.push(now_ms);
        if self.crashes.len() > MAX_CRASHES {
            return Decision::Failed;
        }
        let factor = 1u32.checked_shl(self.consecutive).unwrap_or(u32::MAX);
        self.consecutive = self.consecutive.saturating_add(1);
        Decision::RestartAfter(base.saturating_mul(factor).min(MAX_DELAY))
    }

    /// Forget past crashes (on an explicit `fernctl restart`).
    pub fn reset(&mut self) {
        *self = History::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: Duration = Duration::from_millis(500);

    #[test]
    fn simple_services() {
        let mut h = History::default();
        let on = |h: &mut History, r, e, t| h.on_exit(ServiceType::Simple, r, D, e, 0, t);
        assert_eq!(on(&mut h, Restart::No, Exit::Code(0), 1), Decision::Stopped);
        assert_eq!(on(&mut h, Restart::No, Exit::Code(1), 1), Decision::Failed);
        assert_eq!(
            on(&mut h, Restart::OnFailure, Exit::Code(0), 1),
            Decision::Stopped
        );
        assert_eq!(
            on(&mut h, Restart::OnFailure, Exit::Signal(9), 1),
            Decision::RestartAfter(D)
        );
        assert_eq!(
            on(&mut h, Restart::Always, Exit::Code(0), 2),
            Decision::RestartAfter(D * 2)
        );
    }

    #[test]
    fn oneshots() {
        let mut h = History::default();
        let on = |h: &mut History, r, e| h.on_exit(ServiceType::Oneshot, r, D, e, 0, 1);
        assert_eq!(on(&mut h, Restart::No, Exit::Code(0)), Decision::Done);
        assert_eq!(on(&mut h, Restart::No, Exit::Code(3)), Decision::Failed);
        assert_eq!(
            on(&mut h, Restart::OnFailure, Exit::Code(3)),
            Decision::RestartAfter(D)
        );
    }

    #[test]
    fn backoff_caps_and_resets_when_stable() {
        let mut h = History::default();
        let mut delays = Vec::new();
        for t in 0..8 {
            match h.on_exit(
                ServiceType::Simple,
                Restart::Always,
                D,
                Exit::Code(1),
                t,
                t + 1,
            ) {
                Decision::RestartAfter(d) => delays.push(d),
                d => panic!("{d:?}"),
            }
        }
        assert_eq!(delays[0], D);
        assert_eq!(delays[3], D * 8);
        assert_eq!(*delays.last().unwrap(), MAX_DELAY);

        // Up for a minute before crashing: back to the base delay.
        let start = 100_000;
        let d = h.on_exit(
            ServiceType::Simple,
            Restart::Always,
            D,
            Exit::Code(1),
            start,
            start + 60_000,
        );
        assert_eq!(d, Decision::RestartAfter(D));
    }

    #[test]
    fn crash_loop_gives_up() {
        let mut h = History::default();
        let mut last = Decision::Done;
        for t in 0..=u64::try_from(MAX_CRASHES).unwrap() {
            last = h.on_exit(
                ServiceType::Simple,
                Restart::Always,
                D,
                Exit::Code(1),
                t,
                t + 1,
            );
        }
        assert_eq!(last, Decision::Failed);
        h.reset();
        assert_eq!(
            h.on_exit(ServiceType::Simple, Restart::Always, D, Exit::Code(1), 0, 1),
            Decision::RestartAfter(D)
        );
    }

    #[test]
    fn crashes_outside_the_window_are_forgotten() {
        let mut h = History::default();
        let window = u64::try_from(CRASH_WINDOW.as_millis()).unwrap();
        for i in 0..50u64 {
            // One crash every window/5: never more than ~5 inside a window.
            let t = i * (window / 5) + 1;
            let d = h.on_exit(
                ServiceType::Simple,
                Restart::Always,
                D,
                Exit::Code(1),
                t - 1,
                t,
            );
            assert!(matches!(d, Decision::RestartAfter(_)), "{i}: {d:?}");
        }
    }
}
