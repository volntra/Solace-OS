//! Text for what the core draws itself (window titles, the window
//! manager's overlay): Inter (HIG: Type), rasterized with `ab_glyph` into
//! premultiplied ARGB buffers, one line at a time.
//!
//! Fonts are looked up in `FERN_FONTS`, then `/usr/share/fonts/inter`
//! (the `inter` package), then `build/fonts` (`just fonts`, development).
//! Without them, text is simply not drawn.

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};

/// The weights the core uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    SemiBold,
}

#[derive(Debug, Clone, Default)]
pub struct Fonts {
    regular: Option<FontArc>,
    semibold: Option<FontArc>,
}

/// A rendered line: premultiplied ARGB8888 (little-endian: B, G, R, A).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub pixels: Vec<u8>,
    pub width: i32,
    pub height: i32,
}

impl Fonts {
    /// Loads Inter from the first directory that has it.
    pub fn load() -> Fonts {
        let dirs: Vec<PathBuf> = std::env::var_os("FERN_FONTS")
            .map(PathBuf::from)
            .into_iter()
            .chain([
                PathBuf::from("/usr/share/fonts/inter"),
                PathBuf::from("build/fonts"),
            ])
            .collect();
        for dir in &dirs {
            let regular = load(&dir.join("Inter-Regular.ttf"));
            let semibold = load(&dir.join("Inter-SemiBold.ttf"));
            if regular.is_some() || semibold.is_some() {
                tracing::info!(dir = %dir.display(), "fonts");
                return Fonts { regular, semibold };
            }
        }
        tracing::warn!("Inter not found ({dirs:?}): no titles or overlay text");
        Fonts::default()
    }

    pub fn available(&self) -> bool {
        self.regular.is_some() || self.semibold.is_some()
    }

    fn font(&self, weight: Weight) -> Option<&FontArc> {
        match weight {
            Weight::Regular => self.regular.as_ref().or(self.semibold.as_ref()),
            Weight::SemiBold => self.semibold.as_ref().or(self.regular.as_ref()),
        }
    }

    /// The width `text` takes at `size` pixels.
    pub fn measure(&self, text: &str, size: f32, weight: Weight) -> f32 {
        self.font(weight)
            .map_or(0.0, |f| advance(&f.as_scaled(PxScale::from(size)), text))
    }

    /// Renders one line in `rgba` (straight alpha), shortened with an
    /// ellipsis to fit `max_width` pixels. `None` without fonts or text.
    pub fn line(
        &self,
        text: &str,
        size: f32,
        weight: Weight,
        rgba: [f32; 4],
        max_width: f32,
    ) -> Option<Line> {
        let font = self.font(weight)?;
        let scaled = font.as_scaled(PxScale::from(size));
        let text = fit(&scaled, text, max_width);
        if text.is_empty() {
            return None;
        }
        let ascent = scaled.ascent();
        #[allow(clippy::cast_possible_truncation)]
        let width = advance(&scaled, &text).ceil() as i32 + 2;
        #[allow(clippy::cast_possible_truncation)]
        let height = (ascent - scaled.descent()).ceil() as i32 + 2;
        let (cols, rows) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
        let mut coverage = vec![0f32; cols * rows];
        let mut pen = 1.0;
        let mut prev = None;
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                pen += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scaled.scale(), point(pen, 1.0 + ascent));
            pen += scaled.h_advance(id);
            prev = Some(id);
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let b = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                #[allow(clippy::cast_possible_truncation)]
                let (px, py) = (
                    b.min.x as i64 + i64::from(gx),
                    b.min.y as i64 + i64::from(gy),
                );
                if let (Ok(px), Ok(py)) = (usize::try_from(px), usize::try_from(py))
                    && px < cols
                    && py < rows
                {
                    let slot = &mut coverage[py * cols + px];
                    *slot = (*slot + cov).min(1.0);
                }
            });
        }
        let [r, g, b, a] = rgba;
        let mut pixels = Vec::with_capacity(cols * rows * 4);
        for cov in coverage {
            let alpha = cov * a;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let px = |v: f32| (v * alpha * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels.extend([px(b), px(g), px(r), px(1.0)]);
        }
        Some(Line {
            pixels,
            width,
            height,
        })
    }
}

fn load(path: &Path) -> Option<FontArc> {
    let data = std::fs::read(path).ok()?;
    FontArc::try_from_vec(data).ok()
}

fn advance<F: Font, S: ScaleFont<F>>(scaled: &S, text: &str) -> f32 {
    let mut x = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, id);
        }
        x += scaled.h_advance(id);
        prev = Some(id);
    }
    x
}

/// `text`, or as much of it as fits in `max` followed by an ellipsis.
fn fit<F: Font, S: ScaleFont<F>>(scaled: &S, text: &str, max: f32) -> String {
    if advance(scaled, text) <= max {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
        if advance(scaled, &candidate) <= max {
            return candidate;
        }
    }
    String::new()
}
