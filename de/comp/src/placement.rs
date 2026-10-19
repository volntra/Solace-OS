//! Where a new window goes: centred horizontally, a little above the
//! middle (the eye's resting point), cascading down-right when that spot
//! is taken. A stand-in until sol-wm (Phase 5) owns placement.

/// Cascade step, in logical pixels.
const CASCADE: i32 = 28;

/// The top-left corner for a window of `size` in `area` (x, y, w, h),
/// avoiding the top-left corners in `taken`.
pub fn place(area: (i32, i32, i32, i32), size: (i32, i32), taken: &[(i32, i32)]) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    let (w, h) = size;
    // Centre horizontally; vertically, 40% of the free space above.
    let base_x = ax + ((aw - w) / 2).max(0);
    let base_y = ay + ((ah - h) * 2 / 5).max(0);
    let mut pos = (base_x, base_y);
    for _ in 0..32 {
        if !taken.contains(&pos) {
            break;
        }
        pos = (pos.0 + CASCADE, pos.1 + CASCADE);
        // Wrap back to the area's corner once the window would leave it.
        if pos.0 + w > ax + aw || pos.1 + h > ay + ah {
            pos = (ax + CASCADE, ay + CASCADE);
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: (i32, i32, i32, i32) = (0, 0, 2560, 1440);

    #[test]
    fn first_window_is_centred_a_little_high() {
        assert_eq!(place(AREA, (800, 600), &[]), (880, 336));
    }

    #[test]
    fn taken_spots_cascade() {
        let first = place(AREA, (800, 600), &[]);
        let second = place(AREA, (800, 600), &[first]);
        assert_eq!(second, (first.0 + 28, first.1 + 28));
        let third = place(AREA, (800, 600), &[first, second]);
        assert_eq!(third, (first.0 + 56, first.1 + 56));
    }

    #[test]
    fn windows_larger_than_the_area_start_at_its_corner() {
        assert_eq!(place((100, 50, 640, 480), (1000, 900), &[]), (100, 50));
    }

    #[test]
    fn cascading_wraps_inside_the_area() {
        let area = (0, 0, 400, 300);
        let size = (300, 200);
        let mut taken = Vec::new();
        for _ in 0..10 {
            let p = place(area, size, &taken);
            assert!(p.0 >= 0 && p.1 >= 0 && p.0 + size.0 <= 400 + 28 && p.1 + size.1 <= 300 + 28);
            taken.push(p);
        }
    }
}
