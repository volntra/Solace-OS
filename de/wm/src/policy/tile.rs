//! Edge and corner tiling (CLAUDE.md 6.6): drag a window's titlebar to a
//! screen edge and a preview shows where it will go; let go and it fills
//! that half or quarter. Dragging it away gives back the size it had.
//! Super+arrows do the same from the keyboard.

use super::{Cmd, Rect, Wm, motion, state};

/// Where a window can snap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// The whole work area, inside the gaps (the top edge).
    Fill,
}

/// Space around and between tiled windows, logical pixels.
pub const GAP: i32 = 8;
/// The pointer this close to a screen edge snaps.
const EDGE: i32 = 4;
/// ... and this close to a corner along that edge, to a quarter.
const CORNER: i32 = 96;
/// The preview: Leaf (HIG: Brand) over the desktop, rounded like a window.
const PREVIEW_RGBA: u32 = 0x7C_C4_8A_40;
const PREVIEW_RADIUS: i32 = 12;

/// Every tiled state bit: the window's edges all meet something.
const TILED: u32 = state::TILED_LEFT | state::TILED_RIGHT | state::TILED_TOP | state::TILED_BOTTOM;

impl Tile {
    /// The frame (titlebar included) this tile gives in `area`.
    pub fn rect(self, area: Rect) -> Rect {
        let half_w = area.w / 2;
        let half_h = area.h / 2;
        // Two columns and two rows with GAP around and between them.
        let left = (area.x + GAP, half_w - GAP - GAP / 2);
        let right = (area.x + half_w + GAP / 2, area.w - half_w - GAP - GAP / 2);
        let top = (area.y + GAP, half_h - GAP - GAP / 2);
        let bottom = (area.y + half_h + GAP / 2, area.h - half_h - GAP - GAP / 2);
        let full_w = (area.x + GAP, area.w - 2 * GAP);
        let full_h = (area.y + GAP, area.h - 2 * GAP);
        let (cx, cy) = match self {
            Tile::Left => (left, full_h),
            Tile::Right => (right, full_h),
            Tile::TopLeft => (left, top),
            Tile::TopRight => (right, top),
            Tile::BottomLeft => (left, bottom),
            Tile::BottomRight => (right, bottom),
            Tile::Fill => (full_w, full_h),
        };
        Rect::new(cx.0, cy.0, cx.1, cy.1)
    }

    /// The tile under a pointer at (`px`, `py`) on `screen`, if it is at
    /// an edge (the bottom edge belongs to the dock).
    pub fn at(screen: Rect, px: f64, py: f64) -> Option<Tile> {
        #[allow(clippy::cast_possible_truncation)]
        let (px, py) = (px.floor() as i32, py.floor() as i32);
        let left = px < screen.x + EDGE;
        let right = px >= screen.x + screen.w - EDGE;
        let top = py < screen.y + EDGE;
        let near_top = py < screen.y + CORNER;
        let near_bottom = py >= screen.y + screen.h - CORNER;
        let near_left = px < screen.x + CORNER;
        let near_right = px >= screen.x + screen.w - CORNER;
        Some(match () {
            () if (left && near_top) || (top && near_left) => Tile::TopLeft,
            () if (right && near_top) || (top && near_right) => Tile::TopRight,
            () if left && near_bottom => Tile::BottomLeft,
            () if right && near_bottom => Tile::BottomRight,
            () if left => Tile::Left,
            () if right => Tile::Right,
            () if top => Tile::Fill,
            () => return None,
        })
    }
}

impl Wm {
    /// The screen under a point, if any.
    fn screen_at(&self, px: f64, py: f64) -> Option<Rect> {
        #[allow(clippy::cast_possible_truncation)]
        let (x, y) = (px.floor() as i32, py.floor() as i32);
        self.outputs
            .values()
            .find(|o| x >= o.x && x < o.x + o.w && y >= o.y && y < o.y + o.h)
            .copied()
    }

    /// The window is being dragged by its titlebar; the pointer is at
    /// (`px`, `py`). A tiled or zoomed window takes back its own size
    /// under the pointer; the preview follows the pointer's snap zone.
    pub(super) fn drag_update(&mut self, id: u32, px: f64, py: f64, out: &mut Vec<Cmd>) {
        self.dragged_out_of_tile(id, out);
        let snap = self
            .screen_at(px, py)
            .and_then(|s| Tile::at(s, px, py).map(|t| (t, s)));
        if snap.map(|s| s.0) == self.snap.map(|s| s.0) {
            return;
        }
        self.snap = snap;
        out.push(Cmd::OverlayClear);
        if let Some((tile, screen)) = snap {
            out.push(Cmd::OverlayBeneath(id));
            let r = tile.rect(self.work_area(&screen));
            out.push(Cmd::OverlayRect {
                x: r.x,
                y: r.y,
                w: r.w,
                h: r.h,
                radius: PREVIEW_RADIUS,
                rgba: PREVIEW_RGBA,
            });
        }
    }

    /// The drag ended: snap if the pointer was in a zone.
    pub(super) fn drag_end(&mut self, id: u32, out: &mut Vec<Cmd>) {
        if let Some((tile, screen)) = self.snap.take() {
            out.push(Cmd::OverlayClear);
            let area = self.work_area(&screen);
            self.tile(id, tile, area, out);
        }
    }

    /// A tiled or zoomed window being dragged gets its free size back.
    /// The core keeps the pointer at the same place across its titlebar
    /// as the width changes, so the window stays in hand.
    fn dragged_out_of_tile(&mut self, id: u32, out: &mut Vec<Cmd>) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        let free = match (win.tile, win.untiled, win.unzoomed) {
            (Some(_), Some(u), _) => u,
            (None, _, Some(z)) => Rect::new(z.x, z.y, z.w, z.h - win.tb),
            _ => return,
        };
        win.tile = None;
        win.untiled = None;
        win.unzoomed = None;
        win.states &= !(TILED | state::MAXIMIZED);
        win.cfg = (free.w, free.h);
        out.push(Cmd::Configure {
            id,
            w: free.w,
            h: free.h,
            states: win.states,
        });
    }

    /// Tiles `id` into `tile` of `area`, remembering its free frame.
    pub fn tile(&mut self, id: u32, tile: Tile, area: Rect, out: &mut Vec<Cmd>) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        if win.windowed.is_some() {
            return;
        }
        if win.untiled.is_none() {
            let (w, h) = win.size.unwrap_or(win.cfg);
            let free = Rect::new(win.x, win.y, w, h);
            win.untiled = Some(
                win.unzoomed
                    .map_or(free, |z| Rect::new(z.x, z.y, z.w, z.h - win.tb)),
            );
        }
        win.unzoomed = None;
        win.tile = Some(tile);
        let r = tile.rect(area);
        let w = r.w.max(win.min.0);
        let h = (r.h - win.tb).max(win.min.1);
        win.cfg = (w, h);
        win.x = r.x;
        win.y = r.y;
        win.states = (win.states & !state::MAXIMIZED) | TILED;
        out.push(Cmd::Configure {
            id,
            w,
            h,
            states: win.states,
        });
        out.push(Cmd::Place {
            id,
            x: r.x,
            y: r.y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH,
        });
    }

    /// Puts a tiled window back where and how big it was. Returns whether
    /// it was tiled.
    pub fn untile(&mut self, id: u32, out: &mut Vec<Cmd>) -> bool {
        let Some(win) = self.windows.get_mut(&id) else {
            return false;
        };
        let (Some(_), Some(free)) = (win.tile.take(), win.untiled.take()) else {
            return false;
        };
        win.states &= !TILED;
        win.cfg = (free.w, free.h);
        win.x = free.x;
        win.y = free.y;
        out.push(Cmd::Configure {
            id,
            w: free.w,
            h: free.h,
            states: win.states,
        });
        out.push(Cmd::Place {
            id,
            x: free.x,
            y: free.y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH,
        });
        true
    }

    /// Super+arrow on the focused window: left and right halves, up
    /// fills, down returns a tiled window to its own frame. From a half,
    /// up and down make the matching quarter.
    pub(super) fn tile_key(&mut self, key: TileKey, out: &mut Vec<Cmd>) {
        let Some(id) = self.focused else {
            return;
        };
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        let area = self.work_area(&win.rect());
        let next = match (key, win.tile) {
            (TileKey::Up, Some(Tile::Left)) => Some(Tile::TopLeft),
            (TileKey::Up, Some(Tile::Right)) => Some(Tile::TopRight),
            (TileKey::Down, Some(Tile::Left)) => Some(Tile::BottomLeft),
            (TileKey::Down, Some(Tile::Right)) => Some(Tile::BottomRight),
            (TileKey::Left, _) => Some(Tile::Left),
            (TileKey::Right, _) => Some(Tile::Right),
            (TileKey::Up, _) => Some(Tile::Fill),
            (TileKey::Down, _) => None,
        };
        match next {
            Some(t) => self.tile(id, t, area, out),
            None => {
                self.untile(id, out);
            }
        }
    }
}

/// The tiling keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKey {
    Left,
    Right,
    Up,
    Down,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0,
        y: 0,
        w: 1000,
        h: 700,
    };

    #[test]
    fn halves_and_quarters_split_the_area_with_even_gaps() {
        let l = Tile::Left.rect(SCREEN);
        let r = Tile::Right.rect(SCREEN);
        assert_eq!(l, Rect::new(8, 8, 488, 684));
        assert_eq!(r, Rect::new(504, 8, 488, 684));
        assert_eq!(r.x - (l.x + l.w), GAP);
        assert_eq!(SCREEN.w - (r.x + r.w), GAP);
        let tl = Tile::TopLeft.rect(SCREEN);
        let bl = Tile::BottomLeft.rect(SCREEN);
        assert_eq!(bl.y - (tl.y + tl.h), GAP);
        assert_eq!(SCREEN.h - (bl.y + bl.h), GAP);
        assert_eq!(Tile::Fill.rect(SCREEN), Rect::new(8, 8, 984, 684));
    }

    #[test]
    fn edges_and_corners_choose_the_tile() {
        let at = |x, y| Tile::at(SCREEN, x, y);
        assert_eq!(at(0.0, 350.0), Some(Tile::Left));
        assert_eq!(at(999.0, 350.0), Some(Tile::Right));
        assert_eq!(at(0.0, 10.0), Some(Tile::TopLeft));
        assert_eq!(at(10.0, 0.0), Some(Tile::TopLeft));
        assert_eq!(at(999.0, 690.0), Some(Tile::BottomRight));
        assert_eq!(at(500.0, 0.0), Some(Tile::Fill));
        assert_eq!(at(500.0, 699.0), None, "the bottom edge is the dock's");
        assert_eq!(at(500.0, 350.0), None);
    }
}
