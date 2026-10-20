//! Overview (CLAUDE.md 6.6): every window of the Space on screen springs
//! into a grid, the real windows, scaled, with their titles under them,
//! and a strip of the Spaces runs along the top (the wallpaper with
//! pictures of each Space's windows). Click a window to bring it forward,
//! a Space to go there, the + to add one; anywhere else, Escape or
//! Ctrl+Super+Up again goes back. Arrow keys and Return work too.

use super::{Cmd, Rect, Wm, mods, motion};

/// What Overview shows: where each window went, and the strip.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overview {
    /// The screen it is drawn on.
    pub screen: Rect,
    /// Each window on screen and the frame it is shown in (scaled).
    pub tiles: Vec<(u32, Rect, f64)>,
    /// Each Space's thumbnail, left to right (by index in `Wm::spaces`).
    pub thumbs: Vec<Rect>,
    /// The "new Space" button.
    pub plus: Rect,
    /// The highlighted window (pointer or arrow keys), by tile index.
    pub selected: Option<usize>,
}

/// Space around each tile, and the room under it for its title.
const PAD: i32 = 16;
const LABEL: i32 = 24;
/// The strip: margin above, gap between thumbnails, room for labels.
const STRIP_TOP: i32 = 16;
const STRIP_GAP: i32 = 24;
const STRIP_LABEL: i32 = 22;
/// Colours (HIG: Brand): Ink dims the desktop, Leaf marks the current
/// Space and the highlighted window, Paper for text.
const DIM: u32 = 0x10_15_12_73;
const LEAF: u32 = 0x7C_C4_8A_FF;
const PAPER: u32 = 0xF6_F7_F3_FF;
const PLUS_FILL: u32 = 0xF6_F7_F3_33;

impl Wm {
    /// Ctrl+Super+Up: in or out.
    pub(super) fn toggle_overview(&mut self, out: &mut Vec<Cmd>) {
        if self.overview.is_some() {
            self.leave_overview(None, out);
        } else {
            self.enter_overview(out);
        }
    }

    fn enter_overview(&mut self, out: &mut Vec<Cmd>) {
        out.push(Cmd::GrabPointer(true));
        out.push(Cmd::GrabKeyboard(true));
        self.overview = Some(Overview::default());
        self.layout_overview(out);
    }

    /// Lays out (again) and draws Overview for the windows on screen now.
    pub(super) fn layout_overview(&mut self, out: &mut Vec<Cmd>) {
        let Some(previous) = self.overview.clone() else {
            return;
        };
        let shown: Vec<u32> = self
            .stack
            .iter()
            .copied()
            .filter(|i| self.windows.get(i).is_some_and(|w| w.visible && w.mapped()))
            .collect();
        let screen = shown.first().and_then(|i| self.windows.get(i)).map_or_else(
            || {
                self.outputs
                    .values()
                    .next()
                    .copied()
                    .unwrap_or(Rect::new(0, 0, 1280, 800))
            },
            |w| self.output_of(&w.rect()),
        );
        // The strip: one thumbnail per Space, the screen's shape.
        let th = (screen.h / 8).max(48);
        let tw = th * screen.w / screen.h.max(1);
        let n = i32::try_from(self.spaces.len()).unwrap_or(1);
        let plus_w = th * 2 / 3;
        let total = n * tw + n * STRIP_GAP + plus_w;
        let mut x = screen.x + (screen.w - total) / 2;
        let y = screen.y + STRIP_TOP;
        let mut thumbs = Vec::new();
        for _ in 0..n {
            thumbs.push(Rect::new(x, y, tw, th));
            x += tw + STRIP_GAP;
        }
        let plus = Rect::new(x, y + (th - plus_w) / 2, plus_w, plus_w);
        // The grid, below the strip.
        let top = y + th + STRIP_LABEL + PAD;
        let area = Rect::new(
            screen.x + 3 * PAD,
            top,
            screen.w - 6 * PAD,
            screen.y + screen.h - top - 2 * PAD,
        );
        let frames: Vec<(u32, Rect)> = shown
            .iter()
            .filter_map(|&i| self.windows.get(&i).map(|w| (i, w.rect())))
            .collect();
        let tiles = grid(area, &frames);
        let selected = previous
            .selected
            .and_then(|s| previous.tiles.get(s))
            .and_then(|(id, ..)| tiles.iter().position(|(t, ..)| t == id));
        self.overview = Some(Overview {
            screen,
            tiles,
            thumbs,
            plus,
            selected,
        });
        if let Some(o) = &self.overview {
            for &(id, r, scale) in &o.tiles {
                out.push(Cmd::Place {
                    id,
                    x: r.x,
                    y: r.y,
                    scale,
                    alpha: 1.0,
                    motion: motion::SMOOTH,
                });
            }
        }
        self.draw_overview(out);
    }

    /// The overlay: dimmed desktop, the strip, titles, the highlight. It
    /// lies under the windows, which are the tiles themselves.
    // One drawing command after another: long, but a list.
    #[allow(clippy::too_many_lines)]
    fn draw_overview(&self, out: &mut Vec<Cmd>) {
        let Some(o) = &self.overview else {
            return;
        };
        out.push(Cmd::OverlayClear);
        let bottom = o.tiles.first().map(|t| t.0);
        // Tiles are stacked in grid order; the lowest one in the stack.
        let lowest = self
            .stack
            .iter()
            .copied()
            .find(|i| o.tiles.iter().any(|t| t.0 == *i));
        if let Some(id) = lowest.or(bottom) {
            out.push(Cmd::OverlayBeneath(id));
        }
        let s = o.screen;
        out.push(Cmd::OverlayRect {
            x: s.x,
            y: s.y,
            w: s.w,
            h: s.h,
            radius: 0,
            rgba: DIM,
        });
        let mut desktop = 0;
        for (i, (t, space)) in o.thumbs.iter().zip(&self.spaces).enumerate() {
            if i == self.current {
                out.push(Cmd::OverlayRect {
                    x: t.x - 3,
                    y: t.y - 3,
                    w: t.w + 6,
                    h: t.h + 6,
                    radius: 11,
                    rgba: LEAF,
                });
            }
            out.push(Cmd::OverlayWallpaper {
                x: t.x,
                y: t.y,
                w: t.w,
                h: t.h,
                radius: 8,
            });
            // Its windows in miniature, bottom to top.
            let k = f64::from(t.h) / f64::from(s.h.max(1));
            #[allow(clippy::cast_possible_truncation)]
            let at = |v: i32| (f64::from(v) * k).round() as i32;
            for id in &self.stack {
                let Some(w) = self.windows.get(id) else {
                    continue;
                };
                if w.space != space.id || w.minimized || !w.mapped() {
                    continue;
                }
                let (cw, ch) = w.size.unwrap_or((0, 0));
                let tb = w.rect().h - ch;
                out.push(Cmd::OverlayMirror {
                    id: *id,
                    x: t.x + at(w.x - s.x),
                    y: t.y + at(w.y + tb - s.y),
                    w: at(cw).max(1),
                    h: at(ch).max(1),
                });
            }
            let label = if let Some(w) = space.fullscreen.and_then(|w| self.windows.get(&w)) {
                w.title.clone()
            } else {
                desktop += 1;
                format!("Desktop {desktop}")
            };
            out.push(Cmd::OverlayText {
                x: t.x,
                y: t.y + t.h + 4,
                width: t.w,
                size: 12,
                rgba: PAPER,
                align: 1,
                text: label,
            });
        }
        out.push(Cmd::OverlayRect {
            x: o.plus.x,
            y: o.plus.y,
            w: o.plus.w,
            h: o.plus.h,
            radius: 8,
            rgba: PLUS_FILL,
        });
        out.push(Cmd::OverlayText {
            x: o.plus.x,
            y: o.plus.y + o.plus.h / 2 - 16,
            width: o.plus.w,
            size: 26,
            rgba: PAPER,
            align: 1,
            text: "+".into(),
        });
        for (i, (id, r, _)) in o.tiles.iter().enumerate() {
            if Some(i) == o.selected {
                out.push(Cmd::OverlayRect {
                    x: r.x - 5,
                    y: r.y - 5,
                    w: r.w + 10,
                    h: r.h + 10,
                    radius: 14,
                    rgba: LEAF,
                });
            }
            if let Some(w) = self.windows.get(id) {
                out.push(Cmd::OverlayText {
                    x: r.x - PAD,
                    y: r.y + r.h + 6,
                    width: r.w + 2 * PAD,
                    size: 13,
                    rgba: PAPER,
                    align: 1,
                    text: w.title.clone(),
                });
            }
        }
    }

    /// Back to the desktop: every window springs home; `focus` comes
    /// forward.
    pub(super) fn leave_overview(&mut self, focus: Option<u32>, out: &mut Vec<Cmd>) {
        let Some(o) = self.overview.take() else {
            return;
        };
        for (id, ..) in &o.tiles {
            if let Some(w) = self.windows.get(id) {
                out.push(Cmd::Place {
                    id: *id,
                    x: w.x,
                    y: w.y,
                    scale: 1.0,
                    alpha: 1.0,
                    motion: motion::SMOOTH,
                });
            }
        }
        out.push(Cmd::OverlayClear);
        out.push(Cmd::GrabPointer(false));
        out.push(Cmd::GrabKeyboard(false));
        if let Some(id) = focus {
            self.restore_and_focus(id, out);
        }
    }

    /// A click (on release) while Overview is up.
    pub(super) fn overview_click(&mut self, x: f64, y: f64, pressed: bool, out: &mut Vec<Cmd>) {
        if pressed {
            return;
        }
        let Some(o) = self.overview.clone() else {
            return;
        };
        if let Some((id, ..)) = o.tiles.iter().find(|(_, r, _)| contains(r, x, y)) {
            self.leave_overview(Some(*id), out);
        } else if let Some(i) = o.thumbs.iter().position(|r| contains(r, x, y)) {
            self.leave_overview(None, out);
            self.switch_space(i, None, out);
        } else if contains(&o.plus, x, y) {
            let s = self.new_space(None);
            self.spaces.push(s);
            self.layout_overview(out);
        } else {
            self.leave_overview(None, out);
        }
    }

    /// The pointer moved while Overview is up: highlight what it is over.
    pub(super) fn overview_hover(&mut self, x: f64, y: f64, out: &mut Vec<Cmd>) {
        let Some(o) = self.overview.as_mut() else {
            return;
        };
        let under = o.tiles.iter().position(|(_, r, _)| contains(r, x, y));
        if under != o.selected {
            o.selected = under;
            self.draw_overview(out);
        }
    }

    /// A key while Overview has the keyboard.
    pub(super) fn overview_key(
        &mut self,
        keysym: u32,
        pressed: bool,
        held: u32,
        out: &mut Vec<Cmd>,
    ) {
        const ESCAPE: u32 = 0xff1b;
        const RETURN: u32 = 0xff0d;
        const SPACE: u32 = 0x20;
        const LEFT: u32 = 0xff51;
        const UP: u32 = 0xff52;
        const RIGHT: u32 = 0xff53;
        const DOWN: u32 = 0xff54;
        if !pressed {
            return;
        }
        let Some(o) = self.overview.as_mut() else {
            return;
        };
        let n = o.tiles.len();
        match keysym {
            UP if held & (mods::CTRL | mods::SUPER) == mods::CTRL | mods::SUPER => {
                self.leave_overview(None, out);
            }
            ESCAPE => self.leave_overview(None, out),
            RETURN | SPACE => {
                let id = o.selected.and_then(|s| o.tiles.get(s)).map(|t| t.0);
                self.leave_overview(id, out);
            }
            LEFT | UP | RIGHT | DOWN if n > 0 => {
                let back = matches!(keysym, LEFT | UP);
                o.selected = Some(match (o.selected, back) {
                    (None, _) => 0,
                    (Some(s), true) => (s + n - 1) % n,
                    (Some(s), false) => (s + 1) % n,
                });
                self.draw_overview(out);
            }
            _ => {}
        }
    }
}

fn contains(r: &Rect, x: f64, y: f64) -> bool {
    x >= f64::from(r.x)
        && x < f64::from(r.x + r.w)
        && y >= f64::from(r.y)
        && y < f64::from(r.y + r.h)
}

/// Lays `frames` out in a grid in `area`: the column count that shows the
/// windows largest, rows in reading order of where the windows were, each
/// window scaled to fit its cell (never enlarged) above its title.
// Geometry: x, y, w, h, n read best as they are.
#[allow(clippy::many_single_char_names)]
pub fn grid(area: Rect, frames: &[(u32, Rect)]) -> Vec<(u32, Rect, f64)> {
    let n = frames.len();
    if n == 0 {
        return Vec::new();
    }
    let fit = |f: &Rect, cw: i32, ch: i32| {
        (f64::from(cw - 2 * PAD) / f64::from(f.w.max(1)))
            .min(f64::from(ch - 2 * PAD - LABEL) / f64::from(f.h.max(1)))
            .clamp(0.05, 1.0)
    };
    let mut best = (1, f64::MIN);
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let (Ok(c), Ok(r)) = (i32::try_from(cols), i32::try_from(rows)) else {
            continue;
        };
        let (cw, ch) = (area.w / c, area.h / r);
        let shown: f64 = frames
            .iter()
            .map(|(_, f)| {
                let s = fit(f, cw, ch);
                s * s * f64::from(f.w) * f64::from(f.h)
            })
            .sum();
        if shown > best.1 {
            best = (cols, shown);
        }
    }
    let cols = best.0;
    let rows = n.div_ceil(cols);
    let mut order: Vec<&(u32, Rect)> = frames.iter().collect();
    order.sort_by_key(|(_, f)| (f.y + f.h / 2, f.x + f.w / 2));
    let c = i32::try_from(cols).unwrap_or(1);
    let r = i32::try_from(rows).unwrap_or(1);
    let (cw, ch) = (area.w / c, area.h / r);
    let mut out = Vec::with_capacity(n);
    for (row, chunk) in order.chunks(cols).enumerate() {
        let mut chunk = chunk.to_vec();
        chunk.sort_by_key(|(_, f)| f.x + f.w / 2);
        let k = i32::try_from(chunk.len()).unwrap_or(1);
        // A short last row is centred.
        let left = area.x + (area.w - k * cw) / 2;
        let top = area.y + i32::try_from(row).unwrap_or(0) * ch;
        for (col, (id, f)) in chunk.iter().enumerate() {
            let s = fit(f, cw, ch);
            #[allow(clippy::cast_possible_truncation)]
            let (w, h) = (
                (f64::from(f.w) * s).round() as i32,
                (f64::from(f.h) * s).round() as i32,
            );
            let cx = left + i32::try_from(col).unwrap_or(0) * cw + cw / 2;
            let y = top + PAD + (ch - 2 * PAD - LABEL - h) / 2;
            out.push((*id, Rect::new(cx - w / 2, y, w, h), s));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_keeps_windows_apart_inside_the_area_and_never_enlarges() {
        let area = Rect::new(48, 160, 2464, 1248);
        let frames: Vec<(u32, Rect)> = (1..=5)
            .map(|i| (i, Rect::new(i32::try_from(i).unwrap() * 200, 300, 900, 700)))
            .collect();
        let tiles = grid(area, &frames);
        assert_eq!(tiles.len(), 5);
        for (i, (_, a, s)) in tiles.iter().enumerate() {
            assert!(*s <= 1.0);
            assert!(a.x >= area.x && a.x + a.w <= area.x + area.w, "{a:?}");
            assert!(
                a.y >= area.y && a.y + a.h + LABEL <= area.y + area.h,
                "{a:?}"
            );
            for (_, b, _) in &tiles[i + 1..] {
                assert_eq!(a.overlap(b), 0, "{a:?} {b:?}");
            }
        }
        // One small window is shown at its own size, centred.
        let one = grid(area, &[(1, Rect::new(0, 0, 400, 300))]);
        assert!((one[0].2 - 1.0).abs() < 1e-9);
        assert_eq!(one[0].1.x + 200, area.x + area.w / 2);
    }
}
