//! The app switcher (CLAUDE.md 6.6): Super+Tab shows the apps in the
//! order they were last used, the previous one picked; Tab again (Shift:
//! back) moves along; letting go of Super brings the picked app forward,
//! from whichever Space it is on, minimized or not. Escape cancels.
//! Super+grave (the key above Tab) cycles through the windows of the app
//! in front.
//!
//! Each app shows a live picture of its last window until the icon set
//! (Phase 7) gives it its icon.

use super::{Cmd, Wm, mods};

/// The switcher on screen: apps (app id, its most recent window) and the
/// picked one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Switcher {
    pub apps: Vec<(String, u32)>,
    pub picked: usize,
}

/// One app's cell, its picture, the panel around them (HIG: HUD).
const CELL_W: i32 = 136;
const CELL_H: i32 = 128;
const PICTURE_W: i32 = 112;
const PICTURE_H: i32 = 84;
const PANEL_PAD: i32 = 18;
const PANEL: u32 = 0x10_15_12_D9;
const PICKED: u32 = 0xF6_F7_F3_2E;
const PAPER: u32 = 0xF6_F7_F3_FF;

const TAB: u32 = 0xff09;
const ESCAPE: u32 = 0xff1b;
const SUPER_L: u32 = 0xffeb;
const SUPER_R: u32 = 0xffec;

/// The name to show for an app: the last part of its id, else the title.
pub fn app_name(app_id: &str, title: &str) -> String {
    match app_id.rsplit('.').next() {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => title.to_string(),
    }
}

impl Wm {
    /// Super+Tab (`back`: with Shift).
    pub(super) fn switcher_step(&mut self, back: bool, out: &mut Vec<Cmd>) {
        if let Some(s) = self.switcher.as_mut() {
            let n = s.apps.len();
            s.picked = if back {
                (s.picked + n - 1) % n
            } else {
                (s.picked + 1) % n
            };
            self.draw_switcher(out);
            return;
        }
        if self.overview.is_some() {
            return;
        }
        // Apps by their most recently used window.
        let mut apps: Vec<(String, u32)> = Vec::new();
        for &id in self.mru.iter().rev() {
            if let Some(w) = self.windows.get(&id)
                && w.mapped()
                && !apps.iter().any(|(a, _)| *a == w.app_id)
            {
                apps.push((w.app_id.clone(), id));
            }
        }
        if apps.is_empty() {
            return;
        }
        let n = apps.len();
        let picked = if back { n - 1 } else { 1 % n };
        self.switcher = Some(Switcher { apps, picked });
        out.push(Cmd::GrabKeyboard(true));
        self.draw_switcher(out);
    }

    // Geometry: x, y, w, h, n read best as they are.
    #[allow(clippy::many_single_char_names)]
    fn draw_switcher(&self, out: &mut Vec<Cmd>) {
        let Some(s) = &self.switcher else {
            return;
        };
        let screen = self.focused.and_then(|f| self.windows.get(&f)).map_or_else(
            || self.outputs.values().next().copied().unwrap_or_default(),
            |w| self.output_of(&w.rect()),
        );
        let n = i32::try_from(s.apps.len()).unwrap_or(1);
        let w = n * CELL_W + 2 * PANEL_PAD;
        let h = CELL_H + 2 * PANEL_PAD;
        let x = screen.x + (screen.w - w) / 2;
        let y = screen.y + (screen.h - h) / 2;
        out.push(Cmd::OverlayClear);
        out.push(Cmd::OverlayRect {
            x,
            y,
            w,
            h,
            radius: 18,
            rgba: PANEL,
        });
        for (i, (app, id)) in s.apps.iter().enumerate() {
            let cx = x + PANEL_PAD + i32::try_from(i).unwrap_or(0) * CELL_W;
            let cy = y + PANEL_PAD;
            if i == s.picked {
                out.push(Cmd::OverlayRect {
                    x: cx,
                    y: cy,
                    w: CELL_W,
                    h: CELL_H,
                    radius: 12,
                    rgba: PICKED,
                });
            }
            out.push(Cmd::OverlayMirror {
                id: *id,
                x: cx + (CELL_W - PICTURE_W) / 2,
                y: cy + 10,
                w: PICTURE_W,
                h: PICTURE_H,
            });
            let title = self.windows.get(id).map_or("", |w| w.title.as_str());
            out.push(Cmd::OverlayText {
                x: cx + 6,
                y: cy + 10 + PICTURE_H + 8,
                width: CELL_W - 12,
                size: 12,
                rgba: PAPER,
                align: 1,
                text: app_name(app, title),
            });
        }
    }

    /// A key while the switcher has the keyboard (`held`: modifiers).
    pub(super) fn switcher_key(
        &mut self,
        keysym: u32,
        pressed: bool,
        held: u32,
        out: &mut Vec<Cmd>,
    ) {
        let super_up = (!pressed && matches!(keysym, SUPER_L | SUPER_R)) || held & mods::SUPER == 0;
        if super_up {
            self.switcher_done(true, out);
        } else if pressed && keysym == ESCAPE {
            self.switcher_done(false, out);
        } else if pressed && keysym == TAB {
            self.switcher_step(held & mods::SHIFT != 0, out);
        }
    }

    fn switcher_done(&mut self, go: bool, out: &mut Vec<Cmd>) {
        let Some(s) = self.switcher.take() else {
            return;
        };
        out.push(Cmd::OverlayClear);
        out.push(Cmd::GrabKeyboard(false));
        if !go {
            return;
        }
        let Some((app, id)) = s.apps.get(s.picked).cloned() else {
            return;
        };
        self.bring_app_forward(&app, id, out);
    }

    /// Brings every window of `app` on `id`'s Space forward, `id` on top
    /// and focused.
    fn bring_app_forward(&mut self, app: &str, id: u32, out: &mut Vec<Cmd>) {
        self.restore_and_focus(id, out);
        let space = self.current_space();
        let others: Vec<u32> = self
            .stack
            .iter()
            .copied()
            .filter(|i| {
                *i != id
                    && self
                        .windows
                        .get(i)
                        .is_some_and(|w| w.app_id == app && w.space == space && w.visible)
            })
            .collect();
        self.stack.retain(|i| !others.contains(i) && *i != id);
        self.stack.extend(others);
        self.stack.push(id);
        out.push(Cmd::Stack(self.stack.clone()));
    }

    /// Super+grave: the next window of the app in front, on this Space.
    pub(super) fn cycle_app_windows(&mut self, out: &mut Vec<Cmd>) {
        let Some(front) = self.focused else {
            return;
        };
        let Some(app) = self.windows.get(&front).map(|w| w.app_id.clone()) else {
            return;
        };
        let mine: Vec<u32> = self
            .windows
            .iter()
            .filter(|(_, w)| w.app_id == app && w.visible)
            .map(|(i, _)| *i)
            .collect();
        if mine.len() < 2 {
            return;
        }
        let at = mine.iter().position(|i| *i == front).unwrap_or(0);
        self.focus(Some(mine[(at + 1) % mine.len()]), out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_names_come_from_the_id() {
        assert_eq!(app_name("org.fern.Files", "Home"), "Files");
        assert_eq!(app_name("firefox", "Mozilla Firefox"), "firefox");
        assert_eq!(app_name("", "Untitled"), "Untitled");
    }
}
