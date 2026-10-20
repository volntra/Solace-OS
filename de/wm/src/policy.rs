//! Window policy, free of Wayland: events from the core go in, commands
//! for it come out. Everything fern-wm decides is here, so it is tested
//! without a compositor (`cargo test -p fern-wm`).

use std::collections::BTreeMap;

/// `xdg_toplevel` states (the protocol's `state` bits).
pub mod state {
    pub const ACTIVATED: u32 = 1;
    pub const MAXIMIZED: u32 = 2;
    pub const FULLSCREEN: u32 = 4;
    pub const TILED_LEFT: u32 = 8;
    pub const TILED_RIGHT: u32 = 16;
    pub const TILED_TOP: u32 = 32;
    pub const TILED_BOTTOM: u32 = 64;
}

/// The protocol's `part` values.
pub mod part {
    pub const NONE: u32 = 0;
    pub const CONTENT: u32 = 1;
    pub const TITLEBAR: u32 = 2;
    pub const CLOSE: u32 = 3;
    pub const MINIMIZE: u32 = 4;
    pub const ZOOM: u32 = 5;
    pub const EDGE: u32 = 6;
    pub const OVERLAY: u32 = 7;
}

/// The protocol's `request_kind` values.
pub mod request {
    pub const MOVE: u32 = 1;
    pub const RESIZE: u32 = 2;
    pub const MAXIMIZE: u32 = 3;
    pub const UNMAXIMIZE: u32 = 4;
    pub const FULLSCREEN: u32 = 5;
    pub const UNFULLSCREEN: u32 = 6;
    pub const MINIMIZE: u32 = 7;
    pub const ACTIVATE: u32 = 8;
}

/// Spring presets (the protocol's `motion`).
pub mod motion {
    pub const INSTANT: u32 = 0;
    pub const SNAPPY: u32 = 1;
    pub const SMOOTH: u32 = 2;
    pub const GENTLE: u32 = 3;
}

/// Modifier bits.
pub mod mods {
    pub const SHIFT: u32 = 1;
    pub const CTRL: u32 = 2;
    pub const ALT: u32 = 4;
    pub const SUPER: u32 = 8;
}

/// What the core tells the window manager.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Output {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    OutputRemoved(u32),
    Window {
        id: u32,
        app_id: String,
        title: String,
        parent: u32,
    },
    Title(u32, String),
    AppId(u32, String),
    Hints {
        id: u32,
        min: (i32, i32),
        max: (i32, i32),
    },
    Size {
        id: u32,
        w: i32,
        h: i32,
    },
    Closed(u32),
    Request {
        id: u32,
        kind: u32,
        edges: u32,
    },
    Restore {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        states: u32,
        visible: bool,
        data: String,
    },
    RestoreWm(String),
    Done,
    Button {
        window: u32,
        part: u32,
        x: f64,
        y: f64,
        button: u32,
        pressed: bool,
        mods: u32,
        clicks: u32,
    },
    Motion {
        x: f64,
        y: f64,
    },
    Key {
        keysym: u32,
        pressed: bool,
        mods: u32,
    },
    Binding(u32),
    MoveUpdate {
        id: u32,
        x: i32,
        y: i32,
        px: f64,
        py: f64,
    },
    MoveEnd {
        id: u32,
        x: i32,
        y: i32,
        px: f64,
        py: f64,
    },
    ResizeEnd {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
}

/// What the window manager asks of the core.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Configure {
        id: u32,
        w: i32,
        h: i32,
        states: u32,
    },
    Place {
        id: u32,
        x: i32,
        y: i32,
        scale: f64,
        alpha: f64,
        motion: u32,
    },
    Decoration {
        id: u32,
        titlebar: bool,
    },
    Visible {
        id: u32,
        visible: bool,
    },
    Stack(Vec<u32>),
    Focus(u32),
    Close(u32),
    BeginMove(u32),
    BeginResize(u32, u32),
    Save(u32, String),
    SaveWm(String),
    Bind {
        id: u32,
        mods: u32,
        keysym: u32,
    },
    GrabPointer(bool),
    GrabKeyboard(bool),
    OverlayClear,
    OverlayRect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        rgba: u32,
    },
    OverlayWallpaper {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
    },
    OverlayMirror {
        id: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    OverlayText {
        x: i32,
        y: i32,
        width: i32,
        size: i32,
        rgba: u32,
        align: u32,
        text: String,
    },
}

/// A rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    /// Area shared with `o`.
    pub fn overlap(&self, o: &Rect) -> i64 {
        let w = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let h = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        if w <= 0 || h <= 0 {
            0
        } else {
            i64::from(w) * i64::from(h)
        }
    }
}

/// One window, as the window manager sees it.
#[derive(Debug, Clone, Default)]
pub struct Win {
    pub app_id: String,
    pub title: String,
    pub parent: u32,
    /// Content size the client last committed; `None` until mapped.
    pub size: Option<(i32, i32)>,
    /// Content size we last configured (0: the client chooses).
    pub cfg: (i32, i32),
    pub x: i32,
    pub y: i32,
    pub visible: bool,
    pub states: u32,
    pub min: (i32, i32),
    pub max: (i32, i32),
}

impl Win {
    pub fn mapped(&self) -> bool {
        self.size.is_some()
    }

    /// The frame on screen.
    pub fn rect(&self) -> Rect {
        let (w, h) = self.size.unwrap_or((0, 0));
        Rect::new(self.x, self.y, w, h)
    }
}

/// The window manager's whole state.
#[derive(Debug, Default)]
pub struct Wm {
    pub outputs: BTreeMap<u32, Rect>,
    pub windows: BTreeMap<u32, Win>,
    /// Visible windows, bottom to top.
    pub stack: Vec<u32>,
    pub focused: Option<u32>,
    /// Between bind and `done`: the core is describing the desktop.
    restoring: bool,
}

impl Wm {
    pub fn new() -> Self {
        Wm {
            restoring: true,
            ..Wm::default()
        }
    }

    /// Handles one event; returns what to ask of the core.
    // One arm per event: a table, long by nature.
    #[allow(clippy::too_many_lines)]
    pub fn handle(&mut self, ev: Event) -> Vec<Cmd> {
        let mut out = Vec::new();
        match ev {
            Event::Output { id, x, y, w, h } => {
                self.outputs.insert(id, Rect::new(x, y, w, h));
            }
            Event::OutputRemoved(id) => {
                self.outputs.remove(&id);
            }
            Event::Window {
                id,
                app_id,
                title,
                parent,
            } => {
                self.windows.insert(
                    id,
                    Win {
                        app_id,
                        title,
                        parent,
                        ..Win::default()
                    },
                );
            }
            Event::Title(id, t) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.title = t;
                }
            }
            Event::AppId(id, a) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.app_id = a;
                }
            }
            Event::Hints { id, min, max } => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.min = min;
                    w.max = max;
                }
            }
            Event::Size { id, w, h } => self.on_size(id, w, h, &mut out),
            Event::Closed(id) => self.on_closed(id, &mut out),
            Event::Request { id, kind, .. } => {
                if kind == request::ACTIVATE {
                    self.focus(Some(id), &mut out);
                }
            }
            Event::Restore {
                id,
                x,
                y,
                w,
                h,
                states,
                visible,
                data: _,
            } => {
                if let Some(win) = self.windows.get_mut(&id) {
                    win.x = x;
                    win.y = y;
                    if w > 0 && h > 0 {
                        win.size = Some((w, h));
                    }
                    win.visible = visible;
                    win.states = states;
                    if visible && !self.stack.contains(&id) {
                        self.stack.push(id);
                    }
                    if states & state::ACTIVATED != 0 {
                        self.focused = Some(id);
                    }
                }
            }
            Event::RestoreWm(_) => {}
            Event::Done => {
                self.restoring = false;
                out.push(Cmd::Stack(self.stack.clone()));
                out.push(Cmd::Focus(self.focused.unwrap_or(0)));
            }
            Event::Button {
                window,
                part: _,
                pressed,
                ..
            } => {
                if pressed && window != 0 {
                    self.focus(Some(window), &mut out);
                }
            }
            Event::MoveEnd { id, x, y, .. } | Event::MoveUpdate { id, x, y, .. } => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.x = x;
                    w.y = y;
                }
            }
            Event::ResizeEnd { id, x, y, w, h } => {
                if let Some(win) = self.windows.get_mut(&id) {
                    win.x = x;
                    win.y = y;
                    win.size = Some((w, h));
                    win.cfg = (w, h);
                }
            }
            Event::Motion { .. } | Event::Key { .. } | Event::Binding(_) => {}
        }
        out
    }

    /// The output a window is on (the one it overlaps most), or the first.
    pub fn output_of(&self, r: &Rect) -> Rect {
        self.outputs
            .values()
            .max_by_key(|o| o.overlap(r))
            .copied()
            .unwrap_or(Rect::new(0, 0, 1280, 800))
    }

    fn on_size(&mut self, id: u32, w: i32, h: i32, out: &mut Vec<Cmd>) {
        let first = match self.windows.get_mut(&id) {
            Some(win) => {
                let first = !win.mapped();
                win.size = Some((w, h));
                first
            }
            None => return,
        };
        // While the core describes the desktop after a restart, sizes are
        // facts about placed windows (restore follows), not new windows.
        if !first || self.restoring {
            return;
        }
        let area = self
            .outputs
            .values()
            .next()
            .copied()
            .unwrap_or(Rect::new(0, 0, 1280, 800));
        let others: Vec<Rect> = self
            .stack
            .iter()
            .filter_map(|i| self.windows.get(i))
            .map(Win::rect)
            .collect();
        let (x, y) = place(area, (w, h), &others);
        if let Some(win) = self.windows.get_mut(&id) {
            win.x = x;
            win.y = y;
            win.visible = true;
        }
        out.push(Cmd::Place {
            id,
            x,
            y,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::INSTANT,
        });
        out.push(Cmd::Visible { id, visible: true });
        self.stack.push(id);
        self.focus(Some(id), out);
    }

    fn on_closed(&mut self, id: u32, out: &mut Vec<Cmd>) {
        self.windows.remove(&id);
        self.stack.retain(|&i| i != id);
        if self.focused == Some(id) {
            self.focused = None;
            let next = self.stack.last().copied();
            self.focus(next, out);
        }
    }

    /// Focuses and raises `id` (or nobody), updating activated states.
    pub fn focus(&mut self, id: Option<u32>, out: &mut Vec<Cmd>) {
        let id = id.filter(|i| self.windows.get(i).is_some_and(|w| w.visible));
        if let Some(prev) = self.focused
            && Some(prev) != id
        {
            self.set_states(prev, |s| s & !state::ACTIVATED, out);
        }
        self.focused = id;
        if let Some(id) = id {
            self.set_states(id, |s| s | state::ACTIVATED, out);
            self.stack.retain(|&i| i != id);
            self.stack.push(id);
            out.push(Cmd::Stack(self.stack.clone()));
        }
        out.push(Cmd::Focus(id.unwrap_or(0)));
    }

    fn set_states(&mut self, id: u32, f: impl Fn(u32) -> u32, out: &mut Vec<Cmd>) {
        if let Some(w) = self.windows.get_mut(&id) {
            let s = f(w.states);
            if s != w.states || !self.restoring {
                w.states = s;
                out.push(Cmd::Configure {
                    id,
                    w: w.cfg.0,
                    h: w.cfg.1,
                    states: s,
                });
            }
        }
    }
}

/// Where a new window of `size` goes in `area`: centred, a little above
/// the middle, unless that covers other windows; then the spot that
/// covers them least (ties: the one nearest that first choice).
pub fn place(area: Rect, size: (i32, i32), others: &[Rect]) -> (i32, i32) {
    let (width, height) = size;
    let base = (
        area.x + ((area.w - width) / 2).max(0),
        area.y + ((area.h - height) * 2 / 5).max(0),
    );
    let cost = |px: i32, py: i32| -> i64 {
        let r = Rect::new(px, py, width, height);
        others.iter().map(|o| r.overlap(o)).sum()
    };
    if cost(base.0, base.1) == 0 {
        return base;
    }
    let max_x = area.x + (area.w - width).max(0);
    let max_y = area.y + (area.h - height).max(0);
    // Candidates: a grid, plus every position flush against another
    // window or the area's edges (where the gaps are).
    let step = 24;
    let mut xs: Vec<i32> = (area.x..=max_x).step_by(step).collect();
    let mut ys: Vec<i32> = (area.y..=max_y).step_by(step).collect();
    xs.extend([area.x, max_x, base.0]);
    ys.extend([area.y, max_y, base.1]);
    for o in others {
        xs.extend([o.x + o.w, o.x - width]);
        ys.extend([o.y + o.h, o.y - height]);
    }
    xs.retain(|x| (area.x..=max_x).contains(x));
    ys.retain(|y| (area.y..=max_y).contains(y));
    let mut best = (i64::MAX, i64::MAX, base);
    for &py in &ys {
        for &px in &xs {
            let covered = cost(px, py);
            let dist = i64::from((px - base.0).abs()) + i64::from((py - base.1).abs());
            if (covered, dist) < (best.0, best.1) {
                best = (covered, dist, (px, py));
            }
        }
    }
    best.2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wm_with_output() -> Wm {
        let mut wm = Wm::new();
        wm.handle(Event::Output {
            id: 1,
            x: 0,
            y: 0,
            w: 2560,
            h: 1440,
        });
        wm.handle(Event::Done);
        wm
    }

    fn open(wm: &mut Wm, id: u32, w: i32, h: i32) -> Vec<Cmd> {
        wm.handle(Event::Window {
            id,
            app_id: "app".into(),
            title: "t".into(),
            parent: 0,
        });
        wm.handle(Event::Size { id, w, h })
    }

    #[test]
    fn first_window_is_centred_a_little_high_and_focused() {
        let mut wm = wm_with_output();
        let cmds = open(&mut wm, 1, 800, 600);
        assert!(cmds.contains(&Cmd::Place {
            id: 1,
            x: 880,
            y: 336,
            scale: 1.0,
            alpha: 1.0,
            motion: motion::INSTANT
        }));
        assert!(cmds.contains(&Cmd::Visible {
            id: 1,
            visible: true
        }));
        assert!(cmds.contains(&Cmd::Configure {
            id: 1,
            w: 0,
            h: 0,
            states: state::ACTIVATED
        }));
        assert_eq!(cmds.last(), Some(&Cmd::Focus(1)));
    }

    #[test]
    fn a_second_window_avoids_the_first() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 800, 600);
        open(&mut wm, 2, 800, 600);
        let (a, b) = (wm.windows[&1].rect(), wm.windows[&2].rect());
        assert_eq!(a.overlap(&b), 0, "{a:?} {b:?}");
    }

    #[test]
    fn focus_moves_activation_and_raises() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        open(&mut wm, 2, 400, 300);
        assert_eq!(wm.stack, [1, 2]);
        let cmds = wm.handle(Event::Button {
            window: 1,
            part: part::CONTENT,
            x: 0.0,
            y: 0.0,
            button: 0x110,
            pressed: true,
            mods: 0,
            clicks: 1,
        });
        assert!(cmds.contains(&Cmd::Configure {
            id: 2,
            w: 0,
            h: 0,
            states: 0
        }));
        assert!(cmds.contains(&Cmd::Configure {
            id: 1,
            w: 0,
            h: 0,
            states: state::ACTIVATED
        }));
        assert!(cmds.contains(&Cmd::Stack(vec![2, 1])));
        assert_eq!(wm.focused, Some(1));
    }

    #[test]
    fn closing_the_focused_window_focuses_the_next_one_down() {
        let mut wm = wm_with_output();
        open(&mut wm, 1, 400, 300);
        open(&mut wm, 2, 400, 300);
        let cmds = wm.handle(Event::Closed(2));
        assert_eq!(wm.focused, Some(1));
        assert_eq!(cmds.last(), Some(&Cmd::Focus(1)));
    }

    #[test]
    fn a_restarted_wm_takes_over_the_desktop_as_it_was() {
        let mut wm = Wm::new();
        wm.handle(Event::Output {
            id: 1,
            x: 0,
            y: 0,
            w: 1280,
            h: 800,
        });
        for (id, x, active) in [(4, 10, false), (7, 300, true)] {
            wm.handle(Event::Window {
                id,
                app_id: "a".into(),
                title: "t".into(),
                parent: 0,
            });
            wm.handle(Event::Size { id, w: 200, h: 100 });
            wm.handle(Event::Restore {
                id,
                x,
                y: 20,
                w: 200,
                h: 100,
                states: if active { state::ACTIVATED } else { 0 },
                visible: true,
                data: String::new(),
            });
        }
        let cmds = wm.handle(Event::Done);
        assert_eq!(cmds, [Cmd::Stack(vec![4, 7]), Cmd::Focus(7)]);
        assert_eq!(wm.windows[&4].rect(), Rect::new(10, 20, 200, 100));
    }

    #[test]
    fn placement_prefers_the_least_covered_spot_nearest_the_centre() {
        let area = Rect::new(0, 0, 1000, 800);
        assert_eq!(place(area, (400, 300), &[]), (300, 200));
        let taken = [Rect::new(300, 200, 400, 300)];
        let p = place(area, (400, 300), &taken);
        assert_eq!(Rect::new(p.0, p.1, 400, 300).overlap(&taken[0]), 0);
    }
}
