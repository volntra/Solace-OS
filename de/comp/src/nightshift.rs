//! Night Shift (CLAUDE.md 6.8): warmer colours in the evening, by the
//! displays' gamma ramps (the shell asks with
//! `fern_shell_v1.set_color_temperature`). The ramps scale each channel
//! by the white point of a black body at that temperature, so it costs
//! nothing per frame.

/// Colour temperatures at or above this (and 0) are neutral.
pub const NEUTRAL: u32 = 6500;
/// The warmest the shell may ask for.
pub const WARMEST: u32 = 1000;

/// The white point of a black body at `kelvin`, each channel 0..=1, after
/// Tanner Helland's fit to the CIE 1964 data, relative to 6500 K (which
/// comes out as pure white).
#[must_use]
pub fn white_point(kelvin: u32) -> [f64; 3] {
    if kelvin == 0 || kelvin >= NEUTRAL {
        return [1.0, 1.0, 1.0];
    }
    let t = f64::from(kelvin.max(WARMEST)) / 100.0;
    let fit = |t: f64| -> [f64; 3] {
        let r = if t <= 66.0 {
            255.0
        } else {
            329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
        };
        let g = if t <= 66.0 {
            99.470_802_586_1 * t.ln() - 161.119_568_166_1
        } else {
            288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
        };
        let b = if t >= 66.0 {
            255.0
        } else if t <= 19.0 {
            0.0
        } else {
            138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
        };
        [r, g, b].map(|c| c.clamp(0.0, 255.0) / 255.0)
    };
    let (warm, white) = (fit(t), fit(f64::from(NEUTRAL) / 100.0));
    [0, 1, 2].map(|i| (warm[i] / white[i]).clamp(0.0, 1.0))
}

/// Red, green and blue gamma ramps of `size` entries for `kelvin`.
#[must_use]
pub fn ramps(kelvin: u32, size: u32) -> [Vec<u16>; 3] {
    let wp = white_point(kelvin);
    let last = f64::from(size.saturating_sub(1).max(1));
    wp.map(|scale| {
        (0..size)
            .map(|i| {
                let v = f64::from(i) / last * scale * f64::from(u16::MAX);
                // In range by construction: 0..=1 times u16::MAX.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = v.round() as u16;
                v
            })
            .collect()
    })
}

#[cfg(test)]
// Exact by construction: neutral is a literal, the warmest a clamp.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn neutral_is_identity() {
        for k in [0, NEUTRAL, 9000] {
            assert_eq!(white_point(k), [1.0, 1.0, 1.0]);
            let [r, g, b] = ramps(k, 256);
            assert_eq!(r, g);
            assert_eq!(g, b);
            assert_eq!(r[0], 0);
            assert_eq!(r[255], u16::MAX);
            assert_eq!(r[128], 32896);
        }
    }

    #[test]
    fn warmer_takes_blue_first_then_green() {
        let mut last = [1.0, 1.0, 1.0];
        for k in (1000..6500).rev().step_by(500) {
            let [r, g, b] = white_point(k);
            assert!((r - 1.0).abs() < 1e-9, "red stays at {k}");
            assert!(b <= g && g <= r, "{k}: {r} {g} {b}");
            assert!(g <= last[1] && b <= last[2], "monotonic at {k}");
            last = [r, g, b];
        }
        // Candlelight: little blue left.
        assert!(white_point(1900)[2] < 0.2);
        // Below the warmest is the warmest.
        assert_eq!(white_point(10), white_point(WARMEST));
    }

    #[test]
    fn ramps_are_monotonic_and_scaled() {
        let [r, g, b] = ramps(3400, 1024);
        for ramp in [&r, &g, &b] {
            assert_eq!(ramp.len(), 1024);
            assert!(ramp.windows(2).all(|w| w[0] <= w[1]));
        }
        assert_eq!(r[1023], u16::MAX);
        assert!(b[1023] < g[1023] && g[1023] < u16::MAX);
        assert_eq!(ramps(3400, 0)[0].len(), 0);
        assert_eq!(ramps(3400, 1)[0], vec![0]);
    }
}
