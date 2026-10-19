//! Spring motion (HIG: "critically damped springs, never fixed
//! durations"). A spring is described the way designers tune it: its
//! `response` (the period of the undamped oscillation, in seconds) and its
//! `damping` ratio (1.0 = critically damped: the fastest approach without
//! overshoot). Steps use the closed-form solution of the damped harmonic
//! oscillator, so the result does not depend on the frame rate: two steps
//! of 8 ms land exactly where one step of 16 ms does.

use std::f64::consts::PI;

/// A spring's character.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    /// Seconds per undamped oscillation; smaller is snappier.
    pub response: f64,
    /// 1.0: critically damped (no overshoot); below 1.0 it bounces.
    pub damping: f64,
}

/// Position and velocity of an animated value.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SpringState {
    pub value: f64,
    pub velocity: f64,
}

impl Spring {
    /// Window open/close, general UI motion.
    pub const SMOOTH: Spring = Spring {
        response: 0.42,
        damping: 1.0,
    };
    /// Small, quick changes: hover, focus, toggles.
    pub const SNAPPY: Spring = Spring {
        response: 0.25,
        damping: 1.0,
    };
    /// Large moves across the screen: Overview, Spaces, minimize.
    pub const GENTLE: Spring = Spring {
        response: 0.6,
        damping: 1.0,
    };

    /// Advances `s` towards `target` by `dt` seconds.
    // Short names follow the physics: w (natural frequency), z (damping
    // ratio), u and v (displacement and velocity), r (roots).
    #[allow(clippy::many_single_char_names)]
    pub fn step(&self, s: SpringState, target: f64, dt: f64) -> SpringState {
        if dt <= 0.0 {
            return s;
        }
        let w = 2.0 * PI / self.response.max(1e-3);
        let z = self.damping.max(0.0);
        let u0 = s.value - target;
        let v0 = s.velocity;
        let (u, v) = if (z - 1.0).abs() < 1e-9 {
            let b = v0 + w * u0;
            let e = (-w * dt).exp();
            ((u0 + b * dt) * e, (v0 - w * b * dt) * e)
        } else if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            let e = (-z * w * dt).exp();
            let (sin, cos) = (wd * dt).sin_cos();
            let b = (v0 + z * w * u0) / wd;
            let u = e * (u0 * cos + b * sin);
            let v = e * ((b * wd - z * w * u0) * cos - (u0 * wd + z * w * b) * sin);
            (u, v)
        } else {
            let r = (z * z - 1.0).sqrt();
            let r1 = -w * (z - r);
            let r2 = -w * (z + r);
            let c1 = (v0 - r2 * u0) / (r1 - r2);
            let c2 = u0 - c1;
            let (e1, e2) = ((r1 * dt).exp(), (r2 * dt).exp());
            (c1 * e1 + c2 * e2, r1 * c1 * e1 + r2 * c2 * e2)
        };
        SpringState {
            value: target + u,
            velocity: v,
        }
    }
}

/// A value that follows its target with a spring.
#[derive(Clone, Copy, Debug)]
pub struct Animated {
    pub spring: Spring,
    state: SpringState,
    target: f64,
}

/// Closer than this (in the value's units, and per second) counts as
/// arrived: well below a pixel or a step of opacity.
const SETTLE: f64 = 1e-3;

impl Animated {
    pub fn new(spring: Spring, value: f64) -> Self {
        Animated {
            spring,
            state: SpringState {
                value,
                velocity: 0.0,
            },
            target: value,
        }
    }

    /// Retargets; the current velocity carries over, so interrupted
    /// motion stays continuous.
    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    /// Jumps to `value` (reduce motion, or no animation wanted).
    pub fn snap(&mut self, value: f64) {
        self.target = value;
        self.state = SpringState {
            value,
            velocity: 0.0,
        };
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn value(&self) -> f64 {
        self.state.value
    }

    pub fn is_settled(&self) -> bool {
        (self.state.value - self.target).abs() < SETTLE && self.state.velocity.abs() < SETTLE
    }

    /// Advances by `dt` seconds; returns the new value. Once settled the
    /// value is exactly the target.
    pub fn advance(&mut self, dt: f64) -> f64 {
        self.state = self.spring.step(self.state, self.target, dt);
        if self.is_settled() {
            self.state = SpringState {
                value: self.target,
                velocity: 0.0,
            };
        }
        self.state.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(spring: Spring, from: f64, to: f64, dt: f64, steps: usize) -> Vec<f64> {
        let mut s = SpringState {
            value: from,
            velocity: 0.0,
        };
        (0..steps)
            .map(|_| {
                s = spring.step(s, to, dt);
                s.value
            })
            .collect()
    }

    #[test]
    fn critically_damped_arrives_without_overshoot() {
        for spring in [Spring::SNAPPY, Spring::SMOOTH, Spring::GENTLE] {
            let path = run(spring, 0.0, 100.0, 1.0 / 165.0, 400);
            assert!(path.windows(2).all(|w| w[1] >= w[0] - 1e-9), "monotonic");
            assert!(path.iter().all(|v| *v <= 100.0 + 1e-9), "no overshoot");
            assert!((path.last().unwrap() - 100.0).abs() < 0.01, "arrives");
        }
    }

    #[test]
    fn steps_do_not_depend_on_the_frame_rate() {
        let spring = Spring::SMOOTH;
        let start = SpringState {
            value: 10.0,
            velocity: -40.0,
        };
        let one = spring.step(start, 0.0, 0.016);
        let two = spring.step(spring.step(start, 0.0, 0.008), 0.0, 0.008);
        assert!((one.value - two.value).abs() < 1e-9);
        assert!((one.velocity - two.velocity).abs() < 1e-9);
        // Also for under- and overdamped springs.
        for damping in [0.6, 1.4] {
            let s = Spring {
                response: 0.4,
                damping,
            };
            let one = s.step(start, 0.0, 0.03);
            let three = (0..3).fold(start, |st, _| s.step(st, 0.0, 0.01));
            assert!((one.value - three.value).abs() < 1e-9, "damping {damping}");
        }
    }

    #[test]
    fn underdamped_bounces_and_still_settles() {
        let s = Spring {
            response: 0.4,
            damping: 0.5,
        };
        let path = run(s, 0.0, 1.0, 0.005, 1000);
        assert!(path.iter().any(|v| *v > 1.0), "overshoots");
        assert!((path.last().unwrap() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn a_smaller_response_is_faster() {
        let at = |s: Spring| run(s, 0.0, 1.0, 0.01, 15)[14];
        assert!(at(Spring::SNAPPY) > at(Spring::SMOOTH));
        assert!(at(Spring::SMOOTH) > at(Spring::GENTLE));
    }

    #[test]
    // Settling must land exactly on the target, so exact comparison is the test.
    #[allow(clippy::float_cmp)]
    fn retargeting_keeps_velocity_and_settling_is_exact() {
        let mut a = Animated::new(Spring::SMOOTH, 0.0);
        a.set_target(1.0);
        for _ in 0..10 {
            a.advance(0.01);
        }
        let mid = a.value();
        assert!(mid > 0.0 && mid < 1.0);
        a.set_target(0.0);
        let after = a.advance(0.001);
        assert!(
            after > mid,
            "still moving up for an instant: velocity carried over"
        );
        for _ in 0..1000 {
            a.advance(0.01);
        }
        assert!(a.is_settled());
        assert_eq!(a.value(), 0.0);
        a.snap(5.0);
        assert!(a.is_settled() && a.value() == 5.0);
        assert_eq!(
            a.spring.step(SpringState::default(), 1.0, 0.0),
            SpringState::default()
        );
    }
}
