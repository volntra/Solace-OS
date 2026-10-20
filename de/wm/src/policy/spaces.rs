//! Spaces (CLAUDE.md 6.6): desktops side by side. Switching slides the
//! windows of one out and the next in, the wallpaper staying put. A
//! fullscreen window gets a Space of its own, right after the one it came
//! from, and gives it back when it leaves fullscreen or closes. A Space
//! left empty (other than the last desktop) goes away; carrying a window
//! past the last Space makes a new one.

use super::{Cmd, Rect, Win, Wm, motion, state};

/// One Space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceInfo {
    pub id: u32,
    /// The window this Space belongs to, for a fullscreen Space.
    pub fullscreen: Option<u32>,
}

/// What a fullscreen window goes back to: its Space and frame (content
/// size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Windowed {
    pub space: u32,
    pub frame: Rect,
}

const TILED: u32 = state::TILED_LEFT | state::TILED_RIGHT | state::TILED_TOP | state::TILED_BOTTOM;

impl Wm {
    /// The id of the Space on screen.
    pub fn current_space(&self) -> u32 {
        self.spaces.get(self.current).map_or(1, |s| s.id)
    }

    fn space_index(&self, id: u32) -> Option<usize> {
        self.spaces.iter().position(|s| s.id == id)
    }

    /// The topmost window on screen.
    pub(super) fn top_shown(&self) -> Option<u32> {
        self.stack
            .iter()
            .rev()
            .copied()
            .find(|i| self.windows.get(i).is_some_and(|w| w.visible))
    }

    fn new_space(&mut self, fullscreen: Option<u32>) -> SpaceInfo {
        let s = SpaceInfo {
            id: self.next_space,
            fullscreen,
        };
        self.next_space += 1;
        s
    }

    fn remove_space(&mut self, id: u32) {
        let Some(i) = self.space_index(id) else {
            return;
        };
        self.spaces.remove(i);
        if self.spaces.is_empty() {
            let s = self.new_space(None);
            self.spaces.push(s);
        }
        if i < self.current {
            self.current -= 1;
        }
        self.current = self.current.min(self.spaces.len() - 1);
    }

    /// Shows Space `to`: this Space's windows slide out to one side, the
    /// next one's in from the other. `carry` stays on screen (it is coming
    /// along). Focus goes to `carry`, or the topmost window there.
    pub(super) fn switch_space(&mut self, to: usize, carry: Option<u32>, out: &mut Vec<Cmd>) {
        if to == self.current || to >= self.spaces.len() {
            return;
        }
        let from = self.spaces[self.current];
        let target = self.spaces[to];
        let dir = if to > self.current { 1 } else { -1 };
        let ids: Vec<u32> = self.windows.keys().copied().collect();
        for id in ids {
            if Some(id) == carry {
                continue;
            }
            let Some(rect) = self.windows.get(&id).map(Win::rect) else {
                continue;
            };
            let shift = dir * self.output_of(&rect).w;
            let Some(win) = self.windows.get_mut(&id) else {
                continue;
            };
            if win.minimized || !win.mapped() {
                continue;
            }
            if win.space == from.id && win.visible {
                win.visible = false;
                out.push(Cmd::Place {
                    id,
                    x: win.x - shift,
                    y: win.y,
                    scale: 1.0,
                    alpha: 1.0,
                    motion: motion::SMOOTH,
                });
                out.push(Cmd::Visible { id, visible: false });
            } else if win.space == target.id && !win.visible {
                win.visible = true;
                out.push(Cmd::Place {
                    id,
                    x: win.x + shift,
                    y: win.y,
                    scale: 1.0,
                    alpha: 1.0,
                    motion: motion::INSTANT,
                });
                out.push(Cmd::Visible { id, visible: true });
                out.push(Cmd::Place {
                    id,
                    x: win.x,
                    y: win.y,
                    scale: 1.0,
                    alpha: 1.0,
                    motion: motion::SMOOTH,
                });
            }
        }
        self.current = to;
        // A desktop left empty goes, as long as another desktop remains.
        let desktops = self
            .spaces
            .iter()
            .filter(|s| s.fullscreen.is_none())
            .count();
        // (A window in fullscreen still counts where it will come back.)
        let empty = !self
            .windows
            .values()
            .any(|w| w.space == from.id || w.windowed.is_some_and(|f| f.space == from.id));
        if from.fullscreen.is_none() && desktops > 1 && empty {
            self.remove_space(from.id);
        }
        let next = carry.or_else(|| self.top_shown());
        self.focus(next, out);
    }

    /// Ctrl+Super+Left/Right: the Space to the left or right.
    pub(super) fn switch_by(&mut self, dir: i32, out: &mut Vec<Cmd>) {
        let to = i64::try_from(self.current).unwrap_or(0) + i64::from(dir);
        if let Ok(to) = usize::try_from(to) {
            self.switch_space(to, None, out);
        }
    }

    /// Ctrl+Super+Shift+Left/Right: takes the focused window to the next
    /// desktop that way; past the last one, to a new Space (unless it is
    /// already alone where it is).
    pub(super) fn carry_by(&mut self, dir: i32, out: &mut Vec<Cmd>) {
        let Some(id) = self.focused else {
            return;
        };
        if self.windows.get(&id).is_none_or(|w| w.windowed.is_some()) {
            return;
        }
        let Ok(to) = usize::try_from(i64::try_from(self.current).unwrap_or(0) + i64::from(dir))
        else {
            return;
        };
        if to >= self.spaces.len() {
            let here = self.current_space();
            let alone = !self
                .windows
                .iter()
                .any(|(i, w)| *i != id && w.space == here && !w.minimized);
            if alone {
                return;
            }
            let s = self.new_space(None);
            self.spaces.push(s);
        }
        if self.spaces[to].fullscreen.is_some() {
            return;
        }
        let target = self.spaces[to].id;
        if let Some(win) = self.windows.get_mut(&id) {
            win.space = target;
        }
        self.switch_space(to, Some(id), out);
    }

    /// Fullscreen on (`Some(true)`), off, or toggled (`None`): the window
    /// fills its screen without a titlebar, in a Space of its own next to
    /// the one it came from; off, it goes back where it was.
    pub fn fullscreen(&mut self, id: u32, on: Option<bool>, out: &mut Vec<Cmd>) {
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        let is = win.windowed.is_some();
        let on = on.unwrap_or(!is);
        if on == is || !win.mapped() {
            return;
        }
        if on {
            self.enter_fullscreen(id, out);
        } else {
            self.leave_fullscreen(id, out);
        }
    }

    fn enter_fullscreen(&mut self, id: u32, out: &mut Vec<Cmd>) {
        let Some(rect) = self.windows.get(&id).map(Win::rect) else {
            return;
        };
        let screen = self.output_of(&rect);
        let space = self.new_space(Some(id));
        self.spaces.insert(self.current + 1, space);
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        let (w, h) = win.size.unwrap_or(win.cfg);
        win.windowed = Some(Windowed {
            space: win.space,
            frame: Rect::new(win.x, win.y, w, h),
        });
        win.space = space.id;
        win.tile = None;
        win.untiled = None;
        win.unzoomed = None;
        win.states = (win.states & !(TILED | state::MAXIMIZED)) | state::FULLSCREEN;
        win.cfg = (screen.w, screen.h);
        win.x = screen.x;
        win.y = screen.y;
        out.push(Cmd::Decoration {
            id,
            titlebar: false,
        });
        out.push(Cmd::Configure {
            id,
            w: screen.w,
            h: screen.h,
            states: win.states,
        });
        out.push(Cmd::Place {
            id,
            x: screen.x,
            y: screen.y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH,
        });
        self.switch_space(self.current + 1, Some(id), out);
    }

    fn leave_fullscreen(&mut self, id: u32, out: &mut Vec<Cmd>) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        let Some(back) = win.windowed.take() else {
            return;
        };
        let own = win.space;
        let home = self
            .spaces
            .iter()
            .position(|s| s.id == back.space)
            .or_else(|| self.spaces.iter().position(|s| s.fullscreen.is_none()))
            .unwrap_or(0);
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        win.space = self.spaces.get(home).map_or(1, |s| s.id);
        win.states &= !state::FULLSCREEN;
        win.cfg = (back.frame.w, back.frame.h);
        win.x = back.frame.x;
        win.y = back.frame.y;
        out.push(Cmd::Decoration { id, titlebar: true });
        out.push(Cmd::Configure {
            id,
            w: back.frame.w,
            h: back.frame.h,
            states: win.states,
        });
        out.push(Cmd::Place {
            id,
            x: back.frame.x,
            y: back.frame.y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::SMOOTH,
        });
        self.switch_space(home, Some(id), out);
        self.remove_space(own);
    }

    /// A window is closing: a fullscreen one takes its Space with it (the
    /// desktop it came from comes back first).
    pub(super) fn closing(&mut self, id: u32, out: &mut Vec<Cmd>) {
        let Some(back) = self.windows.get(&id).and_then(|w| w.windowed) else {
            return;
        };
        let Some(own) = self.windows.get(&id).map(|w| w.space) else {
            return;
        };
        if self.current_space() == own {
            let home = self
                .space_index(back.space)
                .or_else(|| self.spaces.iter().position(|s| s.fullscreen.is_none()))
                .unwrap_or(0);
            // The closing window is not carried; it is gone already.
            if let Some(w) = self.windows.get_mut(&id) {
                w.visible = false;
            }
            self.switch_space(home, None, out);
        }
        self.remove_space(own);
    }

    /// The Spaces as `save_wm` data: `spaces=1,2:7,3;current=0;next=4`
    /// (`2:7`: Space 2 belongs to fullscreen window 7).
    pub(super) fn dump_spaces(&self) -> String {
        let spaces: Vec<String> = self
            .spaces
            .iter()
            .map(|s| match s.fullscreen {
                Some(w) => format!("{}:{w}", s.id),
                None => s.id.to_string(),
            })
            .collect();
        format!(
            "spaces={};current={};next={}",
            spaces.join(","),
            self.current,
            self.next_space
        )
    }

    /// Takes back what [`Wm::dump_spaces`] wrote.
    pub(super) fn load_spaces(&mut self, data: &str) {
        let mut spaces = Vec::new();
        for part in data.split(';') {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            match key {
                "spaces" => {
                    for s in value.split(',') {
                        let (id, fs) = s.split_once(':').unwrap_or((s, ""));
                        if let Ok(id) = id.parse() {
                            spaces.push(SpaceInfo {
                                id,
                                fullscreen: fs.parse().ok(),
                            });
                        }
                    }
                }
                "current" => self.current = value.parse().unwrap_or(0),
                "next" => self.next_space = value.parse().unwrap_or(self.next_space),
                _ => {}
            }
        }
        if !spaces.is_empty() {
            self.spaces = spaces;
        }
        self.current = self.current.min(self.spaces.len() - 1);
        let top = self.spaces.iter().map(|s| s.id).max().unwrap_or(0);
        self.next_space = self.next_space.max(top + 1);
    }
}
