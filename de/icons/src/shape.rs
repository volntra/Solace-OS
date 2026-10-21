//! The icon plate: the superellipse from `de/design/shape/squircle.toml`,
//! its shadow, outline and top edge.

use std::f32::consts::TAU;
use std::sync::OnceLock;

use resvg::tiny_skia::{
    Color, FillRule, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform,
};
use serde::Deserialize;

/// The shape's definition (see the file for what each value means).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub exponent: f32,
    pub plate: f32,
    pub shadow_offset: f32,
    pub shadow_blur: f32,
    pub shadow_alpha: f32,
}

/// The shape, as defined once in the design system.
pub fn shape() -> &'static Shape {
    static SHAPE: OnceLock<Shape> = OnceLock::new();
    SHAPE.get_or_init(|| {
        toml::from_str(include_str!("../../design/shape/squircle.toml"))
            .expect("de/design/shape/squircle.toml is valid (tested)")
    })
}

/// The plate's outline, centred in a square of `size` pixels, moved down
/// by `dy`.
#[allow(clippy::many_single_char_names)] // the curve's own names
pub fn plate_path(size: f32, dy: f32) -> Path {
    const POINTS: u16 = 256;
    let s = shape();
    let r = size * s.plate / 2.0;
    let c = size / 2.0;
    let e = 2.0 / s.exponent;
    let mut pb = PathBuilder::new();
    for i in 0..POINTS {
        let t = f32::from(i) / f32::from(POINTS) * TAU;
        let (sin, cos) = t.sin_cos();
        let x = c + r * cos.signum() * cos.abs().powf(e);
        let y = c + dy + r * sin.signum() * sin.abs().powf(e);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    pb.finish().expect("a closed polygon of 256 points")
}

/// The plate as a mask (anti-aliased).
pub fn plate_mask(size: u32, dy: f32) -> Mask {
    let mut mask = Mask::new(size, size).expect("icon sizes are checked");
    #[allow(clippy::cast_precision_loss)]
    let path = plate_path(size as f32, dy);
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    mask
}

/// Three box blurs of `radius` pixels: close to a Gaussian.
fn blur(mask: &mut Mask, radius: usize) {
    if radius == 0 {
        return;
    }
    let w = mask.width() as usize;
    let h = mask.height() as usize;
    let data = mask.data_mut();
    let mut line = vec![0u32; w.max(h)];
    for _ in 0..3 {
        for y in 0..h {
            box_line(data, y * w, 1, w, radius, &mut line);
        }
        for x in 0..w {
            box_line(data, x, w, h, radius, &mut line);
        }
    }
}

/// One box blur along a line of `len` values `stride` apart.
fn box_line(
    data: &mut [u8],
    start: usize,
    stride: usize,
    len: usize,
    radius: usize,
    tmp: &mut [u32],
) {
    for (i, t) in tmp.iter_mut().enumerate().take(len) {
        *t = u32::from(data[start + i * stride]);
    }
    let width = u32::try_from(2 * radius + 1).unwrap_or(u32::MAX);
    let mut sum: u32 = 0;
    // A window over [i - radius, i + radius], outside counted as 0.
    for t in tmp.iter().take(radius.min(len)) {
        sum += t;
    }
    for i in 0..len {
        if i + radius < len {
            sum += tmp[i + radius];
        }
        if i > radius {
            sum -= tmp[i - radius - 1];
        }
        data[start + i * stride] = u8::try_from(sum / width).unwrap_or(u8::MAX);
    }
}

/// Puts a finished plate layer (already clipped to the shape) on a
/// transparent icon of `size`, with its shadow, outline and top edge.
/// `glass` makes the shadow lighter.
pub fn finish(plate: &Pixmap, glass: bool) -> Pixmap {
    let size = plate.width();
    #[allow(clippy::cast_precision_loss)]
    let fsize = size as f32;
    let s = shape();
    let mut out = Pixmap::new(size, size).expect("icon sizes are checked");

    // The shadow.
    let mut shadow = plate_mask(size, fsize * s.shadow_offset);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let radius = (fsize * s.shadow_blur / 2.0).round() as usize;
    blur(&mut shadow, radius);
    let mut paint = Paint::default();
    let alpha = if glass {
        s.shadow_alpha * 0.5
    } else {
        s.shadow_alpha
    };
    paint.set_color(Color::from_rgba(0.0, 0.0, 0.0, alpha).expect("in range"));
    let all = Rect::from_xywh(0.0, 0.0, fsize, fsize).expect("a positive size");
    out.fill_rect(all, &paint, Transform::identity(), Some(&shadow));

    out.draw_pixmap(
        0,
        0,
        plate.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );

    // A faint outline, and a light edge along the top.
    let path = plate_path(fsize, 0.0);
    let width = (fsize / 128.0).max(1.0);
    let mut stroke = Stroke {
        width,
        ..Stroke::default()
    };
    paint.set_color(Color::from_rgba(0.0, 0.0, 0.0, 0.10).expect("in range"));
    out.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    let mut top = Mask::new(size, size).expect("icon sizes are checked");
    top.fill_path(&path, FillRule::Winding, true, Transform::identity());
    // Only the inner half of the stroke, fading out down the plate.
    stroke.width = width * 2.0;
    let fade = resvg::tiny_skia::LinearGradient::new(
        (0.0, 0.0).into(),
        (0.0, fsize * 0.5).into(),
        vec![
            resvg::tiny_skia::GradientStop::new(
                0.0,
                Color::from_rgba(1.0, 1.0, 1.0, 0.45).expect("in range"),
            ),
            resvg::tiny_skia::GradientStop::new(
                1.0,
                Color::from_rgba(1.0, 1.0, 1.0, 0.0).expect("in range"),
            ),
        ],
        resvg::tiny_skia::SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("a valid gradient");
    let edge = Paint {
        shader: fade,
        anti_alias: true,
        ..Paint::default()
    };
    out.stroke_path(&path, &edge, &stroke, Transform::identity(), Some(&top));
    out
}

#[cfg(test)]
// Colours read as #rrggbb; pixel tests name their channels.
#[allow(clippy::unreadable_literal, clippy::many_single_char_names)]
mod tests {
    use super::*;

    #[test]
    fn the_shape_file_is_valid() {
        let s = shape();
        assert!(s.exponent >= 2.0 && s.exponent <= 12.0);
        assert!(s.plate > 0.5 && s.plate <= 1.0);
    }

    #[test]
    fn the_plate_is_squarer_than_a_circle_and_rounder_than_a_square() {
        let mask = plate_mask(200, 0.0);
        let at = |x: usize, y: usize| mask.data()[y * 200 + x];
        // Centre and edge midpoints are inside.
        assert_eq!(at(100, 100), 255);
        assert_eq!(at(100, 16), 255);
        assert_eq!(at(16, 100), 255);
        // The plate's corner is cut off...
        assert_eq!(at(14, 14), 0);
        // ...but less than a circle's would be: a point on the diagonal
        // outside the inscribed circle is inside the plate.
        assert_eq!(at(40, 40), 255);
        // Symmetric.
        assert_eq!(at(30, 50), at(169, 50));
        assert_eq!(at(30, 50), at(30, 149));
    }

    #[test]
    fn finish_adds_a_shadow_below() {
        let mut plate = Pixmap::new(128, 128).unwrap();
        plate.fill(Color::WHITE);
        plate.apply_mask(&plate_mask(128, 0.0));
        let icon = finish(&plate, false);
        // Below the plate's bottom edge there is some shadow; above its
        // top edge, less.
        let alpha = |x: u32, y: u32| icon.pixel(x, y).unwrap().alpha();
        let below = alpha(64, 122);
        let above = alpha(64, 5);
        assert!(below > above, "below {below}, above {above}");
        // The corners stay transparent.
        assert_eq!(alpha(0, 0), 0);
    }
}
