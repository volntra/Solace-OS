//! Frame timing on a real display: from the kernel's presentation
//! timestamps (one per vblank that showed a new frame), how many frames
//! were shown, how evenly, and how many refreshes were missed. Reported
//! every few seconds when `FERN_COMP_STATS` is set; the 165 Hz target of
//! CLAUDE.md 6.5 is checked with it.

use std::time::Duration;

/// How often a report is made.
pub const WINDOW: Duration = Duration::from_secs(5);
/// Gaps longer than this are idle time (nothing changed), not misses.
const IDLE: Duration = Duration::from_millis(250);

/// One report.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub frames: u32,
    /// Frames shown per second over the window.
    pub fps: f64,
    /// Mean and worst time between consecutive frames (idle gaps excluded).
    pub avg: Duration,
    pub max: Duration,
    /// Refreshes that passed without a new frame while frames were coming.
    pub missed: u32,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} frames, {:.1} fps, interval {:.2} ms avg / {:.2} ms max, {} missed refreshes",
            self.frames,
            self.fps,
            self.avg.as_secs_f64() * 1000.0,
            self.max.as_secs_f64() * 1000.0,
            self.missed
        )
    }
}

/// Accumulates presentation times for one display.
#[derive(Debug, Clone)]
pub struct FrameStats {
    refresh: Duration,
    start: Option<Duration>,
    last: Option<Duration>,
    frames: u32,
    intervals: u32,
    sum: Duration,
    max: Duration,
    missed: u32,
}

impl FrameStats {
    pub fn new(refresh: Duration) -> Self {
        FrameStats {
            refresh: refresh.max(Duration::from_micros(100)),
            start: None,
            last: None,
            frames: 0,
            intervals: 0,
            sum: Duration::ZERO,
            max: Duration::ZERO,
            missed: 0,
        }
    }

    /// A frame was shown at `at` (monotonic). Returns a report when a
    /// window is complete.
    pub fn presented(&mut self, at: Duration) -> Option<Report> {
        let start = *self.start.get_or_insert(at);
        if let Some(gap) = self.last.and_then(|last| at.checked_sub(last))
            && !gap.is_zero()
            && gap < IDLE
        {
            self.intervals += 1;
            self.sum += gap;
            self.max = self.max.max(gap);
            // Rounded to whole refreshes: a frame every refresh is 1.
            let periods = (gap.as_secs_f64() / self.refresh.as_secs_f64()).round();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let periods = periods.max(1.0) as u32;
            self.missed += periods - 1;
        }
        self.last = Some(at);
        self.frames += 1;
        let elapsed = at.saturating_sub(start);
        if elapsed < WINDOW {
            return None;
        }
        let report = Report {
            frames: self.frames,
            fps: f64::from(self.frames) / elapsed.as_secs_f64(),
            avg: if self.intervals == 0 {
                Duration::ZERO
            } else {
                self.sum / self.intervals
            },
            max: self.max,
            missed: self.missed,
        };
        *self = FrameStats {
            start: Some(at),
            last: Some(at),
            ..FrameStats::new(self.refresh)
        };
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(stats: &mut FrameStats, times_us: impl IntoIterator<Item = u64>) -> Vec<Report> {
        times_us
            .into_iter()
            .filter_map(|t| stats.presented(Duration::from_micros(t)))
            .collect()
    }

    #[test]
    fn a_steady_165_hz_misses_nothing() {
        let period = 1_000_000 / 165;
        let mut s = FrameStats::new(Duration::from_micros(period));
        let reports = run(&mut s, (0..=166 * 5).map(|i| i * period));
        assert_eq!(reports.len(), 1);
        let r = &reports[0];
        assert!((r.fps - 165.0).abs() < 1.0, "{r}");
        assert_eq!(r.missed, 0);
        assert_eq!(r.avg, Duration::from_micros(period));
    }

    #[test]
    fn dropped_frames_count_as_missed_refreshes_but_idle_does_not() {
        let period = 16_667;
        let mut s = FrameStats::new(Duration::from_micros(period));
        // Every other refresh for a second: one missed each time.
        let mut t: Vec<u64> = (0..30).map(|i| i * 2 * period).collect();
        // Then two seconds of nothing (idle), then steady frames.
        let resume = t.last().unwrap() + 2_000_000;
        t.extend((0..300).map(|i| resume + i * period));
        let r = &run(&mut s, t)[0];
        assert_eq!(r.missed, 29, "{r}");
        assert!(r.max >= Duration::from_micros(2 * period - 1) && r.max < IDLE);
    }

    #[test]
    fn windows_restart_after_each_report() {
        let period = 10_000;
        let mut s = FrameStats::new(Duration::from_micros(period));
        let reports = run(&mut s, (0..=1_100).map(|i| i * period));
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|r| (r.fps - 100.0).abs() < 1.0));
    }
}
