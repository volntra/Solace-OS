//! The five render modes, for native icons and legacy ones.

// Pixel work: sizes are at most 1024 and channels 0-255, so the casts
// between them and f32 are exact or deliberately rounded.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use material_colors::color::Rgb;
use resvg::tiny_skia::{
    ColorU8, FilterQuality, GradientStop, LinearGradient, Paint, Pixmap, PixmapPaint, Rect,
    SpreadMode, Transform,
};
use resvg::usvg;

use crate::Error;
use crate::color::{self, dominant, hct, lightness, skia, split};
use crate::format::{Icon, recolor};
use crate::shape;

/// How icons look (docs/formats/icon.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Default,
    Dark,
    Tinted,
    Wallpaper,
    Clear,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::Default,
        Mode::Dark,
        Mode::Tinted,
        Mode::Wallpaper,
        Mode::Clear,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Default => "default",
            Mode::Dark => "dark",
            Mode::Tinted => "tinted",
            Mode::Wallpaper => "wallpaper",
            Mode::Clear => "clear",
        }
    }

    /// Whether the accent changes how an icon looks in this mode.
    pub fn uses_accent(self) -> bool {
        matches!(self, Mode::Tinted | Mode::Wallpaper)
    }

    fn accented(self) -> bool {
        matches!(self, Mode::Tinted | Mode::Wallpaper | Mode::Clear)
    }
}

impl FromStr for Mode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Mode::ALL
            .into_iter()
            .find(|m| m.name() == s)
            .ok_or_else(|| Error::BadRequest(format!("no icon mode {s:?}")))
    }
}

fn pixmap(size: u32) -> Pixmap {
    Pixmap::new(size, size).expect("icon sizes are checked")
}

/// An SVG layer drawn over the whole `size` square.
fn svg(text: &str, size: u32) -> Result<Pixmap, Error> {
    let tree = usvg::Tree::from_str(text, &usvg::Options::default())
        .map_err(|e| Error::BadIcon(e.to_string()))?;
    let mut out = pixmap(size);
    let s = tree.size();
    let f = size as f32;
    resvg::render(
        &tree,
        Transform::from_scale(f / s.width(), f / s.height()),
        &mut out.as_mut(),
    );
    Ok(out)
}

/// A vertical gradient from `top` to `bottom` over the square.
fn gradient(size: u32, top: Rgb, bottom: Rgb, alpha: (f32, f32)) -> Pixmap {
    let mut out = pixmap(size);
    let f = size as f32;
    let shader = LinearGradient::new(
        (0.0, 0.0).into(),
        (0.0, f).into(),
        vec![
            GradientStop::new(0.0, skia(top, alpha.0)),
            GradientStop::new(1.0, skia(bottom, alpha.1)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("a valid gradient");
    let paint = Paint {
        shader,
        ..Paint::default()
    };
    out.fill_rect(
        Rect::from_xywh(0.0, 0.0, f, f).expect("positive"),
        &paint,
        Transform::identity(),
        None,
    );
    out
}

/// The Tinted and Wallpaper background: dark, in the accent's hue.
fn tinted_background(size: u32, accent: u32) -> Pixmap {
    let (h, c, _) = split(color::rgb(accent));
    let c = c.min(28.0);
    gradient(size, hct(h, c, 30.0), hct(h, c * 0.85, 13.0), (1.0, 1.0))
}

/// The light accent tone a tinted foreground is painted in.
fn tint_colour(accent: u32) -> Rgb {
    let (h, c, _) = split(color::rgb(accent));
    hct(h, c.min(60.0), 86.0)
}

/// Frosted glass: translucent white, lighter at the top.
fn glass_background(size: u32) -> Pixmap {
    let white = Rgb::new(255, 255, 255);
    gradient(size, white, white, (0.50, 0.24))
}

/// Dark mode's background when the icon has no dark layer: near black,
/// with a hint of the key hue.
fn dark_background(size: u32, key: u32) -> Pixmap {
    let (h, c, _) = split(color::rgb(key));
    let c = c.min(10.0);
    gradient(size, hct(h, c, 21.0), hct(h, c, 9.0), (1.0, 1.0))
}

/// Dark mode's role colours: the designer's (`[dark]`) where given;
/// otherwise lightness clamped to stay readable on a dark plate, and a
/// neutral role (a white glyph) takes the key's hue.
fn dark_roles(
    roles: &BTreeMap<String, u32>,
    designed: &BTreeMap<String, u32>,
    key: u32,
) -> BTreeMap<String, u32> {
    let (kh, kc, _) = split(color::rgb(key));
    roles
        .iter()
        .map(|(name, &c)| {
            if let Some(&d) = designed.get(name) {
                return (name.clone(), d);
            }
            let (h, chroma, t) = split(color::rgb(c));
            let out = if chroma < 10.0 {
                // A grey icon's glyph stays grey.
                hct(kh, if kc < 10.0 { kc } else { kc.max(36.0) }, 80.0)
            } else {
                hct(h, chroma, t.clamp(65.0, 92.0))
            };
            (name.clone(), color::to_u32(out))
        })
        .collect()
}

/// Repaints a layer in one colour, keeping each pixel's lightness as its
/// opacity. `invert` uses darkness instead (for dark glyphs).
fn luminance_mask(layer: &mut Pixmap, c: Rgb, invert: bool) {
    for px in layer.pixels_mut() {
        let a = px.alpha();
        if a == 0 {
            continue;
        }
        let d = px.demultiply();
        let mut l = lightness(d.red(), d.green(), d.blue()).clamp(0.0, 1.0);
        if invert {
            l = 1.0 - l;
        }
        // Shadows and dark detail vanish; mid tones read as cut-outs.
        let k = ((l - 0.2) / 0.75).clamp(0.0, 1.0).powf(1.1);
        let na = (f32::from(a) * k).round() as u8;
        *px = ColorU8::from_rgba(c.red, c.green, c.blue, na).premultiply();
    }
}

/// Mean lightness of a layer's visible pixels.
fn mean_lightness(layer: &Pixmap) -> f32 {
    let (mut sum, mut weight) = (0.0f32, 0.0f32);
    for px in layer.pixels() {
        let a = f32::from(px.alpha()) / 255.0;
        if a > 0.0 {
            let d = px.demultiply();
            sum += lightness(d.red(), d.green(), d.blue()) * a;
            weight += a;
        }
    }
    if weight > 0.0 { sum / weight } else { 1.0 }
}

/// Background and foreground, clipped to the plate, finished with its
/// shadow and edges.
fn compose(background: &Pixmap, foreground: &Pixmap, glass: bool) -> Pixmap {
    let size = background.width();
    let mut plate = background.clone();
    plate.draw_pixmap(
        0,
        0,
        foreground.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    plate.apply_mask(&shape::plate_mask(size, 0.0));
    shape::finish(&plate, glass)
}

/// A native icon in `mode`.
pub fn native(icon: &Icon, mode: Mode, accent: u32, size: u32) -> Result<Pixmap, Error> {
    let designed_fg = |roles: &BTreeMap<String, u32>, text: &str| -> Result<Pixmap, Error> {
        let svg_text = recolor(text, roles).map_err(Error::BadIcon)?;
        svg(&svg_text, size)
    };
    let (background, foreground) = match mode {
        Mode::Default => (
            svg(&icon.background, size)?,
            designed_fg(&icon.roles, &icon.foreground)?,
        ),
        Mode::Dark => {
            let bg = match &icon.dark_background {
                Some(text) => svg(text, size)?,
                None => dark_background(size, icon.key),
            };
            let fg = match &icon.dark_foreground {
                Some(text) => designed_fg(&icon.roles, text)?,
                None => designed_fg(
                    &dark_roles(&icon.roles, &icon.dark, icon.key),
                    &icon.foreground,
                )?,
            };
            (bg, fg)
        }
        Mode::Tinted | Mode::Wallpaper | Mode::Clear => {
            let glass = mode == Mode::Clear;
            let bg = if !icon.tint.background {
                svg(&icon.background, size)?
            } else if glass {
                glass_background(size)
            } else {
                tinted_background(size, accent)
            };
            let mut fg = designed_fg(&icon.roles, &icon.foreground)?;
            if icon.tint.foreground {
                let c = if glass {
                    Rgb::new(255, 255, 255)
                } else {
                    tint_colour(accent)
                };
                luminance_mask(&mut fg, c, false);
            }
            (bg, fg)
        }
    };
    Ok(compose(&background, &foreground, mode == Mode::Clear))
}

/// Reads a legacy icon (PNG, or SVG drawn at `size`).
pub fn load_legacy(path: &Path, size: u32) -> Result<Pixmap, Error> {
    let bytes =
        std::fs::read(path).map_err(|e| Error::BadIcon(format!("{}: {e}", path.display())))?;
    let is_svg = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    if is_svg {
        let text = String::from_utf8(bytes)
            .map_err(|e| Error::BadIcon(format!("{}: {e}", path.display())))?;
        let tree = usvg::Tree::from_str(&text, &usvg::Options::default())
            .map_err(|e| Error::BadIcon(format!("{}: {e}", path.display())))?;
        let s = tree.size();
        let scale = size as f32 / s.width().max(s.height());
        let mut out = pixmap(size);
        let dx = (size as f32 - s.width() * scale) / 2.0;
        let dy = (size as f32 - s.height() * scale) / 2.0;
        resvg::render(
            &tree,
            Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
            &mut out.as_mut(),
        );
        Ok(out)
    } else {
        Pixmap::decode_png(&bytes).map_err(|e| Error::BadIcon(format!("{}: {e}", path.display())))
    }
}

/// Whether a legacy icon already fills its square (its corners are
/// opaque), so it is clipped to the plate rather than put on one.
fn fills_its_square(image: &Pixmap) -> bool {
    let (w, h) = (image.width(), image.height());
    let inset = |n: u32| (n as f32 * 0.08) as u32;
    let corners = [
        (inset(w), inset(h)),
        (w - 1 - inset(w), inset(h)),
        (inset(w), h - 1 - inset(h)),
        (w - 1 - inset(w), h - 1 - inset(h)),
    ];
    corners
        .iter()
        .all(|&(x, y)| image.pixel(x, y).is_some_and(|p| p.alpha() > 200))
}

/// `image` scaled into a centred box `fraction` of the plate.
fn placed(image: &Pixmap, size: u32, fraction: f32) -> Pixmap {
    let mut out = pixmap(size);
    let room = size as f32 * shape::shape().plate * fraction;
    let scale = room / image.width().max(image.height()) as f32;
    let dx = (size as f32 - image.width() as f32 * scale) / 2.0;
    let dy = (size as f32 - image.height() as f32 * scale) / 2.0;
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    out.draw_pixmap(
        0,
        0,
        image.as_ref(),
        &paint,
        Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
        None,
    );
    out
}

/// A legacy icon in `mode` (docs/formats/icon.md, "Legacy icons").
pub fn legacy(image: &Pixmap, mode: Mode, accent: u32, size: u32) -> Pixmap {
    let d = dominant(image).unwrap_or(Rgb::new(128, 128, 128));
    let (h, c, _) = split(d);
    let c = c.min(16.0);
    let full = fills_its_square(image);
    if mode.accented() {
        let glass = mode == Mode::Clear;
        let bg = if glass {
            glass_background(size)
        } else {
            tinted_background(size, accent)
        };
        let mut fg = placed(image, size, 0.70);
        let invert = mean_lightness(&fg) < 0.35;
        let tint = if glass {
            Rgb::new(255, 255, 255)
        } else {
            tint_colour(accent)
        };
        luminance_mask(&mut fg, tint, invert);
        return compose(&bg, &fg, glass);
    }
    if full {
        // Its own art, clipped to the plate.
        let art = placed(image, size, 1.0 / shape::shape().plate);
        return compose(&art, &pixmap(size), false);
    }
    let bg = if mode == Mode::Dark {
        gradient(size, hct(h, c, 28.0), hct(h, c, 16.0), (1.0, 1.0))
    } else {
        gradient(size, hct(h, c, 97.0), hct(h, c, 89.0), (1.0, 1.0))
    };
    compose(&bg, &placed(image, size, 0.70), false)
}

#[cfg(test)]
// Colours read as #rrggbb; pixel tests name their channels.
#[allow(clippy::unreadable_literal, clippy::many_single_char_names)]
mod tests {
    use super::*;
    use std::fs;

    pub fn sample_icon(dir: &Path) -> Icon {
        let icon = dir.join("org.example.Blue.fernicon");
        fs::create_dir_all(&icon).unwrap();
        fs::write(
            icon.join("background.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><rect width="1024" height="1024" fill="#2D64C8"/></svg>"##,
        )
        .unwrap();
        // A white square glyph in the middle.
        fs::write(
            icon.join("foreground.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><rect data-role="primary" x="384" y="384" width="256" height="256" fill="#fff"/></svg>"##,
        )
        .unwrap();
        fs::write(
            icon.join("icon.toml"),
            "format = 1\nkey = \"#2D64C8\"\n[roles]\nprimary = \"#FFFFFF\"\n",
        )
        .unwrap();
        Icon::load(&icon).unwrap()
    }

    fn at(p: &Pixmap, x: u32, y: u32) -> ColorU8 {
        p.pixel(x, y).unwrap().demultiply()
    }

    #[test]
    fn modes_by_name() {
        for m in Mode::ALL {
            assert_eq!(m.name().parse::<Mode>().unwrap(), m);
        }
        assert!("sepia".parse::<Mode>().is_err());
    }

    #[test]
    fn each_mode_looks_as_specified() {
        let dir = tempfile::tempdir().unwrap();
        let icon = sample_icon(dir.path());
        let size = 128;
        let (glyph, plate) = ((64, 64), (24, 64));
        let lum = |c: ColorU8| lightness(c.red(), c.green(), c.blue());

        let d = native(&icon, Mode::Default, 0, size).unwrap();
        assert_eq!(
            (
                at(&d, glyph.0, glyph.1).red(),
                at(&d, glyph.0, glyph.1).blue()
            ),
            (255, 255)
        );
        let p = at(&d, plate.0, plate.1);
        assert!(p.blue() > 150 && p.red() < 80, "blue plate: {p:?}");
        // Outside the plate: transparent.
        assert_eq!(d.pixel(1, 1).unwrap().alpha(), 0);

        // Dark: near-black plate; the white glyph takes the key's hue.
        let k = native(&icon, Mode::Dark, 0, size).unwrap();
        assert!(lum(at(&k, plate.0, plate.1)) < 0.25);
        let g = at(&k, glyph.0, glyph.1);
        assert!(g.blue() > g.red() + 30, "a blue glyph: {g:?}");
        assert!(lum(g) > 0.6);

        // Tinted with orange: a dark orange plate, a light orange glyph.
        let t = native(&icon, Mode::Tinted, 0xB4430F, size).unwrap();
        let tp = at(&t, plate.0, plate.1);
        let tg = at(&t, glyph.0, glyph.1);
        assert!(lum(tp) < 0.4 && tp.red() > tp.blue(), "{tp:?}");
        assert!(lum(tg) > 0.75 && tg.red() > tg.blue(), "{tg:?}");
        // Another accent, another colour; Default ignores the accent.
        let t2 = native(&icon, Mode::Tinted, 0x2D64C8, size).unwrap();
        assert_ne!(t.data(), t2.data());
        assert_eq!(
            native(&icon, Mode::Default, 0xB4430F, size).unwrap().data(),
            d.data()
        );
        // Wallpaper is Tinted with the wallpaper's accent.
        assert_eq!(
            native(&icon, Mode::Wallpaper, 0xB4430F, size)
                .unwrap()
                .data(),
            t.data()
        );

        // Clear: a translucent plate, a white glyph.
        let c = native(&icon, Mode::Clear, 0, size).unwrap();
        let cp = c.pixel(plate.0, plate.1).unwrap();
        assert!(
            cp.alpha() > 30 && cp.alpha() < 200,
            "translucent: {}",
            cp.alpha()
        );
        let cg = at(&c, glyph.0, glyph.1);
        assert!(cg.red() > 240 && cg.blue() > 240);
    }

    #[test]
    fn designed_dark_roles_win() {
        let dir = tempfile::tempdir().unwrap();
        let mut icon = sample_icon(dir.path());
        icon.dark.insert("primary".into(), 0xFFC94D);
        let k = native(&icon, Mode::Dark, 0, 128).unwrap();
        let g = at(&k, 64, 64);
        assert!(
            g.red() > 240 && g.blue() < 120,
            "the designer's yellow: {g:?}"
        );
    }

    #[test]
    fn a_layer_that_is_not_tintable_keeps_its_colours() {
        let dir = tempfile::tempdir().unwrap();
        let mut icon = sample_icon(dir.path());
        icon.tint.background = false;
        let t = native(&icon, Mode::Tinted, 0xB4430F, 128).unwrap();
        let p = at(&t, 24, 64);
        assert!(p.blue() > 150 && p.red() < 80, "still blue: {p:?}");
    }

    #[test]
    fn legacy_icons_get_a_plate_in_their_colour() {
        // A red disc on transparency.
        let mut image = pixmap(64);
        let mut pb = resvg::tiny_skia::PathBuilder::new();
        pb.push_circle(32.0, 32.0, 24.0);
        let mut paint = Paint::default();
        paint.set_color(skia(Rgb::new(210, 40, 40), 1.0));
        image.fill_path(
            &pb.finish().unwrap(),
            &paint,
            resvg::tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
        assert!(!fills_its_square(&image));

        let lum = |c: ColorU8| lightness(c.red(), c.green(), c.blue());
        let d = legacy(&image, Mode::Default, 0, 128);
        let plate = at(&d, 14, 64);
        assert!(
            lum(plate) > 0.85 && plate.red() >= plate.blue(),
            "a pale reddish plate: {plate:?}"
        );
        assert!(at(&d, 64, 64).red() > 180, "the disc in the middle");
        let k = legacy(&image, Mode::Dark, 0, 128);
        assert!(lum(at(&k, 14, 64)) < 0.35);
        // Tinted: the disc becomes the accent's light tone.
        let t = legacy(&image, Mode::Tinted, 0x2D64C8, 128);
        let g = at(&t, 64, 64);
        assert!(g.blue() > g.red(), "{g:?}");

        // A square icon is clipped, not put on a plate.
        let mut square = pixmap(64);
        square.fill(skia(Rgb::new(20, 160, 90), 1.0));
        assert!(fills_its_square(&square));
        let s = legacy(&square, Mode::Default, 0, 128);
        let px = at(&s, 14, 64);
        assert!(
            px.green() > 120 && px.red() < 60,
            "its own colour to the edge: {px:?}"
        );
    }

    #[test]
    fn dark_glyphs_are_inverted_for_the_mask() {
        // A black disc: as a lightness mask it would vanish.
        let mut image = pixmap(64);
        let mut pb = resvg::tiny_skia::PathBuilder::new();
        pb.push_circle(32.0, 32.0, 24.0);
        let mut paint = Paint::default();
        paint.set_color(skia(Rgb::new(0, 0, 0), 1.0));
        image.fill_path(
            &pb.finish().unwrap(),
            &paint,
            resvg::tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
        let c = legacy(&image, Mode::Clear, 0, 128);
        let g = at(&c, 64, 64);
        assert!(
            lightness(g.red(), g.green(), g.blue()) > 0.9,
            "a white disc: {g:?}"
        );
    }
}
