//! The pointer: our own arrow, drawn in code (no image assets): a white
//! body, a dark outline and a soft shadow, antialiased by supersampling.
//! Only the DRM backend draws it; nested, the host draws its own.

/// Logical size of the cursor image (square).
pub const SIZE: i32 = 24;
/// Where the arrow points, in logical pixels from the image's corner.
pub const HOTSPOT: (i32, i32) = (3, 2);

/// The arrow's outline in logical pixels, tip first.
const ARROW: [(f64, f64); 7] = [
    (3.0, 2.0),
    (3.0, 19.0),
    (7.2, 15.2),
    (10.0, 21.4),
    (12.8, 20.2),
    (10.0, 14.0),
    (15.6, 14.0),
];
/// Outline width and the shadow's offset and reach.
const OUTLINE: f64 = 1.3;
const SHADOW_OFFSET: (f64, f64) = (0.0, 1.2);
const SHADOW_REACH: f64 = 2.6;
/// Samples per pixel along each axis (antialiasing).
const N: i32 = 4;

fn inside(p: (f64, f64)) -> bool {
    let mut odd = false;
    let n = ARROW.len();
    for i in 0..n {
        let (a, b) = (ARROW[i], ARROW[(i + n - 1) % n]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            odd = !odd;
        }
    }
    odd
}

/// Distance from `point` to the arrow's edge.
fn edge_distance(point: (f64, f64)) -> f64 {
    let count = ARROW.len();
    (0..count)
        .map(|i| {
            let (start, end) = (ARROW[i], ARROW[(i + 1) % count]);
            let (dx, dy) = (end.0 - start.0, end.1 - start.1);
            let along = (((point.0 - start.0) * dx + (point.1 - start.1) * dy)
                / (dx * dx + dy * dy))
                .clamp(0.0, 1.0);
            (point.0 - start.0 - along * dx).hypot(point.1 - start.1 - along * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Premultiplied colour (r, g, b, a) of one sample point.
fn sample(p: (f64, f64)) -> [f64; 4] {
    if inside(p) {
        if edge_distance(p) < OUTLINE * 0.5 {
            return [0.08, 0.08, 0.1, 1.0];
        }
        return [1.0, 1.0, 1.0, 1.0];
    }
    if edge_distance(p) < OUTLINE * 0.5 {
        return [0.08, 0.08, 0.1, 1.0];
    }
    let s = (p.0 - SHADOW_OFFSET.0, p.1 - SHADOW_OFFSET.1);
    let d = if inside(s) { 0.0 } else { edge_distance(s) };
    let a = 0.28 * (1.0 - d / SHADOW_REACH).max(0.0).powi(2);
    [0.0, 0.0, 0.0, a]
}

/// The cursor image at `scale` (integer), as little-endian ARGB bytes
/// (B, G, R, A), premultiplied; `SIZE * scale` pixels square.
pub fn image(scale: i32) -> Vec<u8> {
    let scale = scale.max(1);
    let px = SIZE * scale;
    let mut out = Vec::with_capacity(usize::try_from(px * px * 4).unwrap_or(0));
    for y in 0..px {
        for x in 0..px {
            let mut acc = [0.0; 4];
            for sy in 0..N {
                for sx in 0..N {
                    let p = (
                        (f64::from(x) + (f64::from(sx) + 0.5) / f64::from(N)) / f64::from(scale),
                        (f64::from(y) + (f64::from(sy) + 0.5) / f64::from(N)) / f64::from(scale),
                    );
                    let c = sample(p);
                    for (a, c) in acc.iter_mut().zip(c) {
                        *a += c;
                    }
                }
            }
            let n = f64::from(N * N);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let byte = |v: f64| (v / n * 255.0).round().clamp(0.0, 255.0) as u8;
            out.extend_from_slice(&[byte(acc[2]), byte(acc[1]), byte(acc[0]), byte(acc[3])]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &[u8], x: i32, y: i32) -> [u8; 4] {
        let i = usize::try_from((y * SIZE + x) * 4).unwrap();
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn the_arrow_is_white_inside_outlined_and_clear_outside() {
        let img = image(1);
        assert_eq!(img.len(), usize::try_from(SIZE * SIZE * 4).unwrap());
        // Inside the body: opaque white.
        assert_eq!(pixel(&img, 5, 10), [255, 255, 255, 255]);
        // Just left of the body: the dark outline.
        let edge = pixel(&img, 2, 10);
        assert!(edge[3] > 150 && edge[0] < 80, "{edge:?}");
        // Far from the arrow: transparent.
        assert_eq!(pixel(&img, 22, 2), [0, 0, 0, 0]);
        // The hotspot is on the arrow.
        assert!(pixel(&img, HOTSPOT.0, HOTSPOT.1)[3] > 100);
    }

    #[test]
    fn premultiplied_and_scaled() {
        let img = image(2);
        assert_eq!(img.len(), usize::try_from(SIZE * SIZE * 16).unwrap());
        for px in img.as_chunks::<4>().0 {
            assert!(px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3], "{px:?}");
        }
    }
}
