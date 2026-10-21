//! Colour in HCT (hue, chroma, tone), through the Material Color
//! Utilities port: the tones the modes use, an image's dominant colour,
//! and the wallpaper's accent.

use std::path::Path;

use material_colors::color::Rgb;
use material_colors::hct::Hct;
use material_colors::quantize::{Quantizer, QuantizerCelebi};
use material_colors::score::Score;
use resvg::tiny_skia::{Color, Pixmap};

/// Fern green, the accent when nothing better is found.
pub const FALLBACK: u32 = 0x002F_7A50;

/// `0xRRGGBB` to the library's colour.
pub fn rgb(value: u32) -> Rgb {
    Rgb::from_u32(value & 0x00FF_FFFF)
}

/// The library's colour to `0xRRGGBB`.
pub fn to_u32(c: Rgb) -> u32 {
    (u32::from(c.red) << 16) | (u32::from(c.green) << 8) | u32::from(c.blue)
}

/// A colour for tiny-skia.
pub fn skia(c: Rgb, alpha: f32) -> Color {
    let mut color = Color::from_rgba8(c.red, c.green, c.blue, 255);
    color.set_alpha(alpha.clamp(0.0, 1.0));
    color
}

/// `#rrggbb` to `0xRRGGBB`.
pub fn parse(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// The same hue and chroma at another tone (0 black to 100 white).
pub fn tone(value: u32, tone: f64) -> u32 {
    let h = Hct::new(rgb(value));
    to_u32(Hct::from(h.get_hue(), h.get_chroma(), tone.clamp(0.0, 100.0)).into())
}

/// A colour from hue, chroma and tone.
pub fn hct(hue: f64, chroma: f64, tone: f64) -> Rgb {
    Hct::from(hue, chroma.max(0.0), tone.clamp(0.0, 100.0)).into()
}

/// Hue, chroma and tone of a colour.
pub fn split(c: Rgb) -> (f64, f64, f64) {
    let h = Hct::new(c);
    (h.get_hue(), h.get_chroma(), h.get_tone())
}

/// Perceived lightness L* (0 to 1) of an sRGB colour.
#[allow(clippy::many_single_char_names)] // the formula's own names
pub fn lightness(r: u8, g: u8, b: u8) -> f32 {
    fn linear(c: u8) -> f32 {
        let c = f32::from(c) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let y = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    let f = if y > 216.0 / 24389.0 {
        y.cbrt()
    } else {
        (24389.0 / 27.0 * y + 16.0) / 116.0
    };
    (116.0 * f - 16.0) / 100.0
}

/// Up to `max` opaque pixels of an image, evenly spread.
fn samples(image: &Pixmap, max: usize) -> Vec<Rgb> {
    let pixels = image.pixels();
    let step = (pixels.len() / max.max(1)).max(1);
    pixels
        .iter()
        .step_by(step)
        .filter(|p| p.alpha() >= 128)
        .map(|p| {
            let c = p.demultiply();
            Rgb::new(c.red(), c.green(), c.blue())
        })
        .collect()
}

/// The colour most of an icon is painted in: the largest cluster with
/// some colour to it, else the largest (a grey icon gets a grey).
pub fn dominant(image: &Pixmap) -> Option<Rgb> {
    let pixels = samples(image, 4096);
    if pixels.is_empty() {
        return None;
    }
    let clusters = QuantizerCelebi::quantize(&pixels, 16).color_to_count;
    let colourful = clusters
        .iter()
        .filter(|(c, _)| split(**c).1 >= 12.0)
        .max_by_key(|(_, n)| **n);
    colourful
        .or_else(|| clusters.iter().max_by_key(|(_, n)| **n))
        .map(|(c, _)| *c)
}

/// The accent a wallpaper suggests: the best-scoring colour of a
/// quantized copy (Material Color Utilities' source colour).
pub fn accent_of(image: &Pixmap) -> u32 {
    let pixels = samples(image, 128 * 128);
    if pixels.is_empty() {
        return FALLBACK;
    }
    let clusters = QuantizerCelebi::quantize(&pixels, 128).color_to_count;
    Score::score(&clusters, Some(1), Some(rgb(FALLBACK)), None)
        .first()
        .map_or(FALLBACK, |c| to_u32(*c))
}

/// The accent of a wallpaper file (PNG), or `None` if it cannot be read.
pub fn wallpaper_accent(path: &Path) -> Option<u32> {
    let image = Pixmap::load_png(path).ok()?;
    Some(accent_of(&image))
}

#[cfg(test)]
// Colours read as #rrggbb; pixel tests name their channels.
#[allow(clippy::unreadable_literal, clippy::many_single_char_names)]
mod tests {
    use super::*;
    use resvg::tiny_skia::{Paint, Rect, Transform};

    fn filled(colors: &[(u32, f32)]) -> Pixmap {
        // Horizontal bands, each a share of the height.
        let mut p = Pixmap::new(64, 100).unwrap();
        let mut y = 0.0;
        for &(c, share) in colors {
            let mut paint = Paint::default();
            paint.set_color(skia(rgb(c), 1.0));
            let h = 100.0 * share;
            p.fill_rect(
                Rect::from_xywh(0.0, y, 64.0, h).unwrap(),
                &paint,
                Transform::identity(),
                None,
            );
            y += h;
        }
        p
    }

    #[test]
    fn parses_colours() {
        assert_eq!(parse("#2F7A50"), Some(0x2F7A50));
        assert_eq!(parse("#2f7a50"), Some(0x2F7A50));
        assert_eq!(parse("2F7A50"), None);
        assert_eq!(parse("#2F7A5"), None);
        assert_eq!(parse("#2F7A5G"), None);
    }

    #[test]
    fn tones_keep_the_hue() {
        let light = tone(0x2D64C8, 90.0);
        let dark = tone(0x2D64C8, 20.0);
        let (h1, _, t1) = split(rgb(light));
        let (h2, _, t2) = split(rgb(dark));
        assert!((t1 - 90.0).abs() < 1.0 && (t2 - 20.0).abs() < 1.0);
        assert!((h1 - h2).abs() < 8.0, "{h1} vs {h2}");
        assert_eq!(tone(0xFFFFFF, 100.0), 0xFFFFFF);
    }

    #[test]
    fn lightness_ends() {
        assert!(lightness(0, 0, 0).abs() < 1e-3);
        assert!((lightness(255, 255, 255) - 1.0).abs() < 1e-3);
        assert!(lightness(0, 0, 255) < lightness(0, 255, 0));
    }

    #[test]
    fn dominant_prefers_colour_over_white() {
        // Mostly white with a red mark: the red, not the white.
        let p = filled(&[(0xFFFFFF, 0.7), (0xD03030, 0.3)]);
        let d = dominant(&p).unwrap();
        assert!(d.red > 180 && d.green < 90, "{d:?}");
        // All grey: grey.
        let g = dominant(&filled(&[(0x808080, 1.0)])).unwrap();
        assert!(split(g).1 < 5.0);
    }

    #[test]
    fn wallpaper_accent_follows_the_picture() {
        let blue = accent_of(&filled(&[(0x1F4FA8, 0.6), (0x0A0A0A, 0.4)]));
        let (hue, chroma, _) = split(rgb(blue));
        assert!(chroma > 20.0, "{blue:06x}");
        let (want, _, _) = split(rgb(0x1F4FA8));
        assert!((hue - want).abs() < 15.0, "{hue} vs {want}");
        // Nothing to go on: Fern green.
        assert_eq!(accent_of(&Pixmap::new(4, 4).unwrap()), FALLBACK);
    }
}
