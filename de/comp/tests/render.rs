//! Pixel tests for fern-comp's renderer: a headless compositor, real
//! Wayland clients (`fern-testclient`), screenshots, and checks on
//! specific pixels. Screenshots are kept in
//! `target/tmp/fern-comp-render/` for inspection.
//!
//! Runs on Mesa's software renderer (llvmpipe) so results do not depend
//! on the GPU; needs the dev shell (`nix develop`) for Mesa's EGL.
//!
//! This test binary has its own `main` (`harness = false`): each
//! scenario starts its own compositor, one at a time, and the process
//! environment is set before any thread exists.

use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zbus::zvariant::Value;

use ferncomp::{
    Comp, LoopData, backdrop,
    backend::headless::{Headless, Image},
    render::{WINDOW_RADIUS, Wallpaper},
};
use smithay::{
    desktop::Window,
    reexports::{calloop::EventLoop, wayland_server::Display},
};

/// The output, and the wallpaper's flat grey (shadows must darken it).
const SIZE: (i32, i32) = (1000, 700);
const GREY: [u8; 3] = [128, 128, 128];

type Result<T = ()> = std::result::Result<T, String>;

struct Harness {
    name: &'static str,
    event_loop: EventLoop<'static, LoopData>,
    data: LoopData,
    headless: Headless,
    clients: Vec<Child>,
    /// Windows in the order their clients were started.
    windows: Vec<Window>,
}

impl Harness {
    /// A compositor with fern-wm running on a thread (each start, after a
    /// crash too, gets a new thread on a new socket pair).
    fn with_wm(name: &'static str) -> Result<Self> {
        let mut h = Self::new(name)?;
        h.data.state.set_wm_launcher(Box::new(|| {
            let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
            std::thread::spawn(move || {
                if let Ok(conn) = wayland_client::Connection::from_socket(theirs) {
                    let _ = fernwm::run(&conn);
                }
            });
            Ok(ours)
        }));
        h.wait_for_wm()?;
        Ok(h)
    }

    fn wait_for_wm(&mut self) -> Result {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.data.state.wm.connected() {
            if Instant::now() > deadline {
                return Err("the window manager never connected".into());
            }
            self.turn()?;
        }
        self.settle()
    }

    fn location(&self, i: usize) -> (i32, i32) {
        let p = self
            .data
            .state
            .space
            .element_location(&self.windows[i])
            .unwrap_or_default();
        (p.x, p.y)
    }

    fn focused(&self, i: usize) -> bool {
        let surface = self.windows[i].toplevel().map(|t| t.wl_surface().clone());
        self.data
            .state
            .seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .is_some_and(|f| Some(f) == surface)
    }

    fn new(name: &'static str) -> Result<Self> {
        let event_loop: EventLoop<LoopData> = EventLoop::try_new().map_err(|e| e.to_string())?;
        let display: Display<Comp> = Display::new().map_err(|e| e.to_string())?;
        let display_handle = display.handle();
        let mut event_loop = event_loop;
        let mut state = Comp::new(&mut event_loop, display).map_err(|e| e.to_string())?;
        let headless = Headless::new(&mut state, SIZE, Some(Wallpaper::solid(GREY)))
            .map_err(|e| format!("headless output (run inside `nix develop`): {e}"))?;
        Ok(Harness {
            name,
            event_loop,
            data: LoopData {
                state,
                display_handle,
            },
            headless,
            clients: Vec::new(),
            windows: Vec::new(),
        })
    }

    /// One turn: client requests, then a frame. Returns whether the frame
    /// changed anything.
    fn turn(&mut self) -> Result<bool> {
        self.event_loop
            .dispatch(Some(Duration::from_millis(2)), &mut self.data)
            .map_err(|e| e.to_string())?;
        let changed = self
            .headless
            .render(&mut self.data.state)
            .map_err(|e| e.to_string())?;
        let _ = self.data.display_handle.flush_clients();
        Ok(changed)
    }

    /// Starts `fern-testclient ARGS` and waits until its window is placed
    /// and shown.
    fn open(&mut self, args: &str) -> Result<Window> {
        let child = Command::new(env!("CARGO_BIN_EXE_fern-testclient"))
            .args(args.split_whitespace())
            .env("WAYLAND_DISPLAY", &self.data.state.socket_name)
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| format!("fern-testclient: {e}"))?;
        self.clients.push(child);
        self.wait_for_window(args)
    }

    /// Starts `fern-testclient ARGS` as a trusted part of the desktop (the
    /// way the core starts the shell): on a socket pair, `WAYLAND_SOCKET`.
    /// Does not wait for its window.
    fn start_trusted(&mut self, args: &str) -> Result {
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().map_err(|e| e.to_string())?;
        self.data
            .state
            .insert_trusted_client(ours)
            .map_err(|e| e.to_string())?;
        let child = Command::new(env!("CARGO_BIN_EXE_fern-testclient"))
            .args(args.split_whitespace())
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
            .env("WAYLAND_SOCKET", "0")
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| format!("fern-testclient: {e}"))?;
        self.clients.push(child);
        Ok(())
    }

    /// Like [`Harness::start_trusted`], with the client's output piped back.
    fn start_trusted_piped(&mut self, args: &str) -> Result<std::process::ChildStdout> {
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().map_err(|e| e.to_string())?;
        self.data
            .state
            .insert_trusted_client(ours)
            .map_err(|e| e.to_string())?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_fern-testclient"))
            .args(args.split_whitespace())
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
            .env("WAYLAND_SOCKET", "0")
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("fern-testclient: {e}"))?;
        let out = child.stdout.take().ok_or("no stdout")?;
        self.clients.push(child);
        Ok(out)
    }

    /// How many layer surfaces (shell parts) are on the output.
    fn layers(&self) -> usize {
        smithay::desktop::layer_map_for_output(self.headless.output()).len()
    }

    fn wait_for_window(&mut self, args: &str) -> Result<Window> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.turn()?;
            let new = self
                .data
                .state
                .space
                .elements()
                .find(|w| !self.windows.contains(w) && w.geometry().size.w > 0);
            if let Some(w) = new.cloned() {
                self.windows.push(w.clone());
                self.settle()?;
                return Ok(w);
            }
            if Instant::now() > deadline {
                return Err(format!("no window from `fern-testclient {args}`"));
            }
        }
    }

    /// Runs turns until nothing has changed for a while (clients answer
    /// configures asynchronously, so a couple of quiet frames is not
    /// enough).
    fn settle(&mut self) -> Result {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut quiet_since = Instant::now();
        while Instant::now() < deadline {
            if self.turn()? {
                quiet_since = Instant::now();
            } else if quiet_since.elapsed() > Duration::from_millis(150) {
                return Ok(());
            }
        }
        Err("the screen never stopped changing".into())
    }

    /// Moves window `i` so its top-left corner is at (x, y), keeping the
    /// stacking order (mapping raises; the windows above are re-raised).
    fn place(&mut self, i: usize, x: i32, y: i32) -> Result {
        let w = self.windows[i].clone();
        let space = &mut self.data.state.space;
        let above: Vec<Window> = space
            .elements()
            .skip_while(|e| **e != w)
            .skip(1)
            .cloned()
            .collect();
        space.map_element(w, (x, y), false);
        for e in &above {
            space.raise_element(e, false);
        }
        self.settle()
    }

    fn focus(&mut self, i: usize) -> Result {
        let w = self.windows[i].clone();
        self.data.state.focus(Some(&w));
        self.settle()
    }

    /// The window's rectangle on screen: (x, y, w, h).
    fn rect(&self, i: usize) -> (i32, i32, i32, i32) {
        let g = self
            .data
            .state
            .space
            .element_geometry(&self.windows[i])
            .unwrap_or_default();
        (g.loc.x, g.loc.y, g.size.w, g.size.h)
    }

    fn shot(&mut self, label: &str) -> Result<Image> {
        let img = self.headless.screenshot().map_err(|e| e.to_string())?;
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fern-comp-render");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        img.save_png(&dir.join(format!("{}-{label}.png", self.name)))?;
        Ok(img)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for c in &mut self.clients {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn luma(p: [u8; 4]) -> f64 {
    0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
}

fn near(p: [u8; 4], rgb: [u8; 3], tolerance: u8) -> bool {
    p.iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= tolerance)
}

/// The widest spread of any channel along a horizontal line.
fn spread(img: &Image, y: i32, x0: i32, x1: i32) -> u8 {
    (0..3)
        .map(|c| {
            let vals: Vec<u8> = (x0..x1).map(|x| img.pixel(x, y)[c]).collect();
            vals.iter().max().unwrap_or(&0) - vals.iter().min().unwrap_or(&0)
        })
        .max()
        .unwrap_or(0)
}

fn check(cond: bool, what: impl FnOnce() -> String) -> Result {
    if cond { Ok(()) } else { Err(what()) }
}

const RED: &str = "--color ff0000ff --size 400x300";

/// The window lands where the compositor says and the image is upright:
/// a window near the top shows near the top of the screenshot.
fn orientation(h: &mut Harness) -> Result {
    h.open(RED)?;
    h.place(0, 100, 40)?;
    let img = h.shot("top")?;
    let p = img.pixel(300, 60);
    check(near(p, [255, 0, 0], 2), || {
        format!("inside the window at the top: {p:?}")
    })?;
    let p = img.pixel(300, 600);
    check(near(p, GREY, 2), || format!("far below the window: {p:?}"))
}

/// Corners are cut along the radius, edges are not; the window casts a
/// shadow below it and nothing is drawn far away.
fn corners_and_shadow(h: &mut Harness) -> Result {
    h.open(RED)?;
    h.place(0, 300, 200)?;
    let img = h.shot("window")?;
    let (x, y, width, height) = h.rect(0);
    // The radius is a whole number of pixels.
    #[allow(clippy::cast_possible_truncation)]
    let radius = WINDOW_RADIUS as i32;
    for (cx, cy, name) in [
        (x, y, "top-left"),
        (x + width - 1, y, "top-right"),
        (x, y + height - 1, "bottom-left"),
        (x + width - 1, y + height - 1, "bottom-right"),
    ] {
        let p = img.pixel(cx, cy);
        check(p[0] < 160, || {
            format!("{name} corner pixel is not cut: {p:?}")
        })?;
    }
    // Along an edge, one radius in from the corner: fully inside.
    let p = img.pixel(x + radius, y);
    check(near(p, [255, 0, 0], 2), || {
        format!("top edge next to the corner: {p:?}")
    })?;
    let p = img.pixel(x + width / 2, y + height / 2);
    check(near(p, [255, 0, 0], 2), || format!("centre: {p:?}"))?;
    // The shadow falls below: darker than the wallpaper, fading out.
    let below = luma(img.pixel(x + width / 2, y + height + 8));
    let further = luma(img.pixel(x + width / 2, y + height + 50));
    let grey = luma([GREY[0], GREY[1], GREY[2], 255]);
    check(below < grey - 20.0, || {
        format!("no shadow under the window: {below} vs {grey}")
    })?;
    check(further > below, || {
        format!("the shadow does not fade: {below} then {further}")
    })?;
    let p = img.pixel(20, 20);
    check(near(p, GREY, 1), || {
        format!("far corner of the screen: {p:?}")
    })
}

/// The focused window casts the darker shadow, and focus moving away
/// lightens it.
fn focus_shadow(h: &mut Harness) -> Result {
    h.open(RED)?;
    h.open("--color 0000ffff --size 300x200")?;
    h.place(0, 60, 100)?;
    h.place(1, 600, 100)?;
    let (x, y, w, hh) = h.rect(0);
    let probe = (x + w / 2, y + hh + 12);
    h.focus(0)?;
    let focused = luma(h.shot("focused")?.pixel(probe.0, probe.1));
    h.focus(1)?;
    let unfocused = luma(h.shot("unfocused")?.pixel(probe.0, probe.1));
    check(focused + 5.0 < unfocused, || {
        format!("focused shadow {focused} is not darker than unfocused {unfocused}")
    })?;
    check(unfocused < 126.0, || {
        format!("an unfocused window casts no shadow: {unfocused}")
    })
}

/// A transparent window that asks for blur shows what is behind it
/// blurred; without the request, sharp. Unchanged backdrops are reused,
/// and moving what is behind re-blurs.
fn blur(h: &mut Harness) -> Result {
    h.open("--stripes --size 700x500")?;
    h.place(0, 150, 100)?;
    h.open("--color ffffff00 --size 400x200 --blur")?;
    h.place(1, 300, 250)?;
    h.open("--color ffffff00 --size 150x100")?;
    h.place(2, 160, 110)?;
    let img = h.shot("glass")?;
    // Under the glass: the stripes are smoothed out.
    let blurred = spread(&img, 350, 360, 640);
    check(blurred < 60, || {
        format!("not blurred under the glass: spread {blurred}")
    })?;
    // Beside it and under the plain transparent window: still sharp.
    let sharp = spread(&img, 550, 200, 800);
    check(sharp > 150, || {
        format!("stripes outside the glass lost contrast: {sharp}")
    })?;
    let plain = spread(&img, 160, 170, 300);
    check(plain > 150, || {
        format!("blurred without asking: spread {plain}")
    })?;
    // A blur this strong averages the four stripe colours (see
    // fern-testclient): no darkening, no tint.
    let p = img.pixel(500, 350);
    check(near(p, [153, 157, 158], 10), || {
        format!("blurred colour {p:?} is not the stripes' average")
    })?;

    // Idle frames reuse the blurred backdrop.
    let before = backdrop::renders();
    for _ in 0..10 {
        h.turn()?;
    }
    let after = backdrop::renders();
    check(after == before, || {
        format!("re-blurred {} times while idle", after - before)
    })?;
    // Moving the striped window behind re-blurs.
    h.place(0, 170, 100)?;
    check(backdrop::renders() > after, || {
        "moving the backdrop did not re-blur".into()
    })
}

/// Only the requested region is blurred.
fn blur_region(h: &mut Harness) -> Result {
    h.open("--stripes --size 700x500")?;
    h.place(0, 150, 100)?;
    h.open("--color ffffff00 --size 400x200 --blur-region 0,0,400,100")?;
    h.place(1, 300, 250)?;
    let img = h.shot("region")?;
    let inside = spread(&img, 290, 360, 640);
    check(inside < 60, || {
        format!("not blurred inside the region: spread {inside}")
    })?;
    let outside = spread(&img, 420, 360, 640);
    check(outside > 150, || {
        format!("blurred outside the region: spread {outside}")
    })
}

/// Once settled, frames draw nothing: idle costs no GPU work.
fn idle_frames_have_no_damage(h: &mut Harness) -> Result {
    h.open(RED)?;
    h.open("--color ffffff00 --size 300x200 --blur")?;
    for i in 0..10 {
        check(!h.turn()?, || {
            format!("frame {i} after settling had damage")
        })?;
    }
    Ok(())
}

type Scenario = (&'static str, fn(&mut Harness) -> Result);

/// Window `i`'s rectangle on screen.
fn rect(h: &Harness, i: usize) -> fernwm::policy::Rect {
    let (left, top) = h.location(i);
    let size = h.windows[i].geometry().size;
    fernwm::policy::Rect::new(left, top, size.w, size.h)
}

fn wm_places_and_focuses(h: &mut Harness) -> Result {
    h.open("--color ff5a5fff --size 300x200")?;
    h.open("--color 3a86ffff --size 300x200")?;
    h.shot("placed")?;
    let (a, b) = (rect(h, 0), rect(h, 1));
    check(a.overlap(&b) == 0, || {
        format!("windows overlap: {a:?} {b:?}")
    })?;
    check(h.focused(1), || "the new window does not have focus".into())?;
    // A click on the first window focuses and raises it.
    let (left, top) = h.location(0);
    let at = smithay::utils::Point::from((f64::from(left) + 10.0, f64::from(top) + 10.0));
    h.data.state.wm_button(at, 0x110, true);
    h.data.state.wm_button(at, 0x110, false);
    h.settle()?;
    check(h.focused(0), || "a click did not move focus".into())?;
    let top = h.data.state.space.elements().last().cloned();
    check(top.as_ref() == Some(&h.windows[0]), || {
        "a click did not raise".into()
    })
}

fn wm_restart_keeps_windows(h: &mut Harness) -> Result {
    h.open("--color ff5a5fff --size 300x200")?;
    h.open("--color 3a86ffff --size 300x200")?;
    let before = (h.location(0), h.location(1));
    h.data.state.kill_wm();
    // A few turns, well inside the restart delay.
    for _ in 0..5 {
        h.turn()?;
    }
    check(!h.data.state.wm.connected(), || {
        "still connected after the kill".into()
    })?;
    check((h.location(0), h.location(1)) == before, || {
        "windows moved when the WM died".into()
    })?;
    h.wait_for_wm()?;
    check((h.location(0), h.location(1)) == before, || {
        "windows moved when the WM came back".into()
    })?;
    check(h.focused(1), || "focus was lost across the restart".into())?;
    // The new window manager knows the old windows: a third avoids them.
    h.open("--color 2ec4b6ff --size 300x200")?;
    h.shot("after-restart")?;
    let c = rect(h, 2);
    for i in 0..2 {
        let o = rect(h, i);
        check(c.overlap(&o) == 0, || {
            format!("new window {c:?} covers {o:?}")
        })?;
    }
    check(h.focused(2), || "the new window does not have focus".into())
}

/// Clicks (press and release) with the left button at (x, y).
fn click(h: &mut Harness, x: f64, y: f64, times: u32) -> Result {
    let at = smithay::utils::Point::from((x, y));
    for _ in 0..times {
        h.data.state.wm_button(at, 0x110, true);
        h.data.state.wm_button(at, 0x110, false);
    }
    h.settle()
}

fn wm_titlebar(h: &mut Harness) -> Result {
    use fern_wm_proto::server::fern_wm_v1::Part;
    h.open("--color 3a86ffff --size 400x260 --title Notes")?;
    let img = h.shot("titlebar")?;
    let (x, y) = h.location(0);
    let tb = ferncomp::render::TITLEBAR_HEIGHT;
    // The titlebar sits above the content: light, with a red close light
    // (the window is focused) and the content below it.
    let bar = img.pixel(x + 200, y - tb + 4);
    check(near(bar, [0xEA, 0xEC, 0xE7], 6), || {
        format!("titlebar colour {bar:?}")
    })?;
    let close = img.pixel(x + 20, y - tb / 2);
    check(near(close, [0xEE, 0x5D, 0x50], 30), || {
        format!("close light {close:?}")
    })?;
    let content = img.pixel(x + 200, y + 10);
    check(near(content, [0x3a, 0x86, 0xff], 4), || {
        format!("content {content:?}")
    })?;
    // The core hits the parts where they are drawn.
    let part_at = |h: &Harness, dx: f64| {
        h.data
            .state
            .hit((f64::from(x) + dx, f64::from(y - tb / 2)).into())
            .map(|hit| hit.part)
    };
    check(part_at(h, 20.0) == Some(Part::Close), || {
        "close not hit".into()
    })?;
    check(part_at(h, 40.0) == Some(Part::Minimize), || {
        "minimize not hit".into()
    })?;
    check(part_at(h, 60.0) == Some(Part::Zoom), || {
        "zoom not hit".into()
    })?;
    check(part_at(h, 200.0) == Some(Part::Titlebar), || {
        "titlebar not hit".into()
    })?;
    // Close acts on release over the button: the client is asked to close
    // and goes away.
    click(h, f64::from(x) + 20.0, f64::from(y - tb / 2), 1)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.data.state.space.elements().next().is_some() {
        check(Instant::now() < deadline, || {
            "the window did not close".into()
        })?;
        h.turn()?;
    }
    Ok(())
}

fn wm_zoom(h: &mut Harness) -> Result {
    h.open("--color 2ec4b6ff --size 300x200 --title Zoom")?;
    let (x, y) = h.location(0);
    let tb = ferncomp::render::TITLEBAR_HEIGHT;
    // A double click on the titlebar fills the screen, content under the
    // titlebar; another one brings it back.
    click(h, f64::from(x) + 150.0, f64::from(y - tb / 2), 2)?;
    let size = h.windows[0].geometry().size;
    check(h.location(0) == (0, tb), || {
        format!("zoomed to {:?}", h.location(0))
    })?;
    check((size.w, size.h) == (SIZE.0, SIZE.1 - tb), || {
        format!("zoomed size {size:?}")
    })?;
    h.shot("zoomed")?;
    let (zx, zy) = h.location(0);
    click(h, f64::from(zx) + 400.0, f64::from(zy - tb / 2), 2)?;
    let size = h.windows[0].geometry().size;
    check(h.location(0) == (x, y), || {
        format!("unzoomed to {:?}", h.location(0))
    })?;
    check((size.w, size.h) == (300, 200), || {
        format!("unzoomed size {size:?}")
    })
}

/// Presses the left button at `from`, moves to `to` in a dozen steps, taking a
/// frame after each, and returns the screenshot taken just before release
/// (labelled `label`) once released and settled.
fn drag(h: &mut Harness, from: (f64, f64), to: (f64, f64), label: &str) -> Result<Image> {
    let steps = 12;
    h.data.state.pointer_to(from.into(), 0);
    h.data.state.pointer_button(0x110, true, 0);
    h.settle()?;
    for i in 1..=steps {
        let t = f64::from(i) / f64::from(steps);
        let at = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        h.data.state.pointer_to(at.into(), i);
        h.turn()?;
    }
    h.settle()?;
    let held = h.shot(label)?;
    h.data.state.pointer_button(0x110, false, 100);
    h.settle()?;
    Ok(held)
}

fn wm_tiling(h: &mut Harness) -> Result {
    use fernwm::policy::{Rect, Tile};
    h.open("--color 2ec4b6ff --size 300x200 --title Tile")?;
    let (x, y) = h.location(0);
    let tb = ferncomp::render::TITLEBAR_HEIGHT;
    let grab = (f64::from(x) + 150.0, f64::from(y - tb / 2));
    // Held at the left edge: the preview (Leaf over grey) shows the left
    // half; let go and the window fills it, content under its titlebar.
    let held = drag(h, grab, (0.0, 350.0), "tile-preview")?;
    let left = Tile::Left.rect(Rect::new(0, 0, SIZE.0, SIZE.1));
    let preview = held.pixel(left.x + 20, left.y + left.h - 20);
    check(preview[1] > 135 && preview[0] < 135, || {
        format!("no green preview at the left half: {preview:?}")
    })?;
    // The preview lies under the window in hand (grabbed 150 px across,
    // so its content spans x -150..150 below the pointer).
    let held_window = held.pixel(100, 350 + tb);
    check(near(held_window, [0x2e, 0xc4, 0xb6], 4), || {
        format!("the preview tints the dragged window: {held_window:?}")
    })?;
    h.shot("tiled")?;
    let size = h.windows[0].geometry().size;
    check(h.location(0) == (left.x, left.y + tb), || {
        format!(
            "tiled at {:?}, want {:?}",
            h.location(0),
            (left.x, left.y + tb)
        )
    })?;
    check((size.w, size.h) == (left.w, left.h - tb), || {
        format!("tiled size {size:?}, want {}x{}", left.w, left.h - tb)
    })?;
    // The preview is gone after the drop.
    check(h.data.state.wm.overlay.is_empty(), || {
        "the preview stayed".into()
    })?;
    // Dragged out to the middle: back to its own size, still held.
    let (tx, ty) = h.location(0);
    let grab = (f64::from(tx) + 100.0, f64::from(ty - tb / 2));
    drag(h, grab, (500.0, 300.0), "untiling")?;
    let size = h.windows[0].geometry().size;
    check((size.w, size.h) == (300, 200), || {
        format!("dragged out at size {size:?}")
    })?;
    let (ux, uy) = h.location(0);
    check(
        (f64::from(ux)..f64::from(ux + 300)).contains(&500.0) && (uy - tb..uy).contains(&300),
        || format!("the pointer lost the window: it is at {:?}", (ux, uy)),
    )
}

/// Presses and releases a key the way the keyboard does (the binding
/// lookup included): `mods` as the protocol's modifier bits.
fn key(h: &mut Harness, mods: u32, keysym: u32) -> Result {
    use smithay::input::keyboard::ModifiersState;
    let state = ModifiersState {
        shift: mods & 1 != 0,
        ctrl: mods & 2 != 0,
        alt: mods & 4 != 0,
        logo: mods & 8 != 0,
        ..ModifiersState::default()
    };
    let keycode = 100;
    let action = h.data.state.key_action(keycode, true, &state, keysym);
    check(action.is_some(), || {
        format!("{keysym:#x} with {mods} is not bound")
    })?;
    if let Some(a) = action {
        h.data.state.wm_key_event(a);
    }
    if let Some(a) = h.data.state.key_action(keycode, false, &state, keysym) {
        h.data.state.wm_key_event(a);
    }
    h.settle()
}

fn shown(h: &Harness, i: usize) -> bool {
    h.data.state.space.elements().any(|w| *w == h.windows[i])
}

fn wm_spaces(h: &mut Harness) -> Result {
    const CTRL_SUPER: u32 = 2 | 8;
    const SHIFT: u32 = 1;
    const LEFT: u32 = 0xff51;
    const RIGHT: u32 = 0xff53;
    h.open("--color ff5a5fff --size 300x200 --title One")?;
    h.open("--color 3a86ffff --size 300x200 --title Two")?;
    let (home0, home1) = (h.location(0), h.location(1));
    // Carry Two to the right: a new Space; One slides away.
    key(h, CTRL_SUPER | SHIFT, RIGHT)?;
    h.shot("space-2")?;
    check(!shown(h, 0) && shown(h, 1), || {
        format!("after carrying: shown {} {}", shown(h, 0), shown(h, 1))
    })?;
    check(h.focused(1), || "the carried window lost focus".into())?;
    check(h.location(1) == home1, || {
        format!("the carried window moved: {:?}", h.location(1))
    })?;
    // Back left: One returns where it was, focused; Two is away.
    key(h, CTRL_SUPER, LEFT)?;
    check(shown(h, 0) && !shown(h, 1), || {
        "the first Space did not come back".into()
    })?;
    check(h.location(0) == home0, || {
        format!("One came back at {:?}", h.location(0))
    })?;
    check(h.focused(0), || "focus did not follow the Space".into())?;
    // Fullscreen: One fills the screen in a Space of its own, without a
    // titlebar; leaving fullscreen puts it back.
    key(h, CTRL_SUPER, 0x66)?;
    let size = h.windows[0].geometry().size;
    let img = h.shot("fullscreen")?;
    check(h.location(0) == (0, 0) && (size.w, size.h) == SIZE, || {
        format!("fullscreen at {:?} size {size:?}", h.location(0))
    })?;
    let corner = img.pixel(2, 2);
    check(near(corner, [0xff, 0x5a, 0x5f], 4), || {
        format!("the fullscreen corner is not the window's: {corner:?}")
    })?;
    key(h, CTRL_SUPER, 0x66)?;
    let size = h.windows[0].geometry().size;
    check(
        h.location(0) == home0 && (size.w, size.h) == (300, 200),
        || format!("left fullscreen at {:?} size {size:?}", h.location(0)),
    )?;
    // Two is still on the Space to the right.
    key(h, CTRL_SUPER, RIGHT)?;
    check(!shown(h, 0) && shown(h, 1), || {
        "Two's Space was lost".into()
    })
}

/// Turns until `cond` holds, or fails with `what` after 10 s.
fn wait_until(h: &mut Harness, what: &str, cond: impl Fn(&Harness) -> bool) -> Result {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond(h) {
        check(Instant::now() < deadline, || format!("timed out: {what}"))?;
        h.turn()?;
    }
    Ok(())
}

fn wm_minimize_to_dock(h: &mut Harness) -> Result {
    // An app cannot see the window list (foreign-toplevel management).
    let mut lister = Command::new(env!("CARGO_BIN_EXE_fern-testclient"))
        .arg("--globals")
        .env("WAYLAND_DISPLAY", &h.data.state.socket_name)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while lister.try_wait().map_err(|e| e.to_string())?.is_none() {
        check(Instant::now() < deadline, || "--globals hung".into())?;
        h.turn()?;
    }
    let mut globals = String::new();
    if let Some(mut out) = lister.stdout.take() {
        use std::io::Read;
        out.read_to_string(&mut globals)
            .map_err(|e| e.to_string())?;
    }
    check(globals.contains("xdg_wm_base"), || {
        format!("no globals listed: {globals}")
    })?;
    check(
        !globals.contains("zwlr_foreign_toplevel_manager_v1"),
        || "an untrusted client sees the window list".into(),
    )?;
    // A window, then a trusted dock that minimizes it into an icon on its
    // own surface (10,10 48x48) and, once minimized, activates it.
    h.open("--color 2ec4b6ff --size 300x200 --title Target")?;
    let home = h.location(0);
    h.start_trusted("--dock Target --title Dock --size 200x100 --color 7cc48aff")?;
    let in_list = |h: &Harness| {
        h.data
            .state
            .wm
            .record(&h.windows[0])
            .is_some_and(|r| r.visible)
    };
    wait_until(h, "the target was not minimized", |h| !in_list(h))?;
    let dock_window = h
        .data
        .state
        .space
        .elements()
        .find(|w| **w != h.windows[0])
        .cloned()
        .ok_or("the dock has no window")?;
    let dock = h
        .data
        .state
        .space
        .element_location(&dock_window)
        .map(|p| (p.x, p.y))
        .ok_or("the dock is not placed")?;
    let place = h
        .data
        .state
        .wm
        .record(&h.windows[0])
        .map(|r| r.place)
        .ok_or("no record")?;
    let size = h.windows[0].geometry().size;
    let tb = f64::from(ferncomp::render::TITLEBAR_HEIGHT);
    let centre = (
        f64::from(place.x) + f64::from(size.w) * place.scale / 2.0,
        f64::from(place.y) + (f64::from(size.h) + tb) * place.scale / 2.0,
    );
    let icon = (f64::from(dock.0) + 34.0, f64::from(dock.1) + 34.0);
    check(
        (centre.0 - icon.0).abs() <= 2.0 && (centre.1 - icon.1).abs() <= 2.0,
        || format!("minimized towards {centre:?}, the icon is at {icon:?}"),
    )?;
    check(place.alpha == 0.0 && place.scale < 0.25, || {
        format!("minimized place {place:?}")
    })?;
    wait_until(h, "the dock's activate did not bring it back", in_list)?;
    h.settle()?;
    check(h.location(0) == home, || {
        format!("came back at {:?}, was at {home:?}", h.location(0))
    })?;
    check(h.focused(0), || "the restored window is not focused".into())?;
    // Super+M minimizes the focused window (no icon for it: towards the
    // bottom of the screen).
    key(h, 8, 0x6d)?;
    check(!shown(h, 0), || "Super+M did not minimize".into())
}

fn wm_overview(h: &mut Harness) -> Result {
    const CTRL_SUPER: u32 = 2 | 8;
    const UP: u32 = 0xff52;
    h.open("--color ff5a5fff --size 360x240 --title Red")?;
    h.open("--color 3a86ffff --size 360x240 --title Blue")?;
    h.open("--color 2ec4b6ff --size 360x240 --title Teal")?;
    let homes: Vec<(i32, i32)> = (0..3).map(|i| h.location(i)).collect();
    key(h, CTRL_SUPER, UP)?;
    let img = h.shot("overview")?;
    let o = wm_overview_state(h)?;
    check(o.tiles.len() == 3, || format!("{} tiles", o.tiles.len()))?;
    // Each tile shows its window (the window itself, scaled down), the
    // desktop around is dimmed, the strip shows the Space.
    let colours = [[0xff, 0x5a, 0x5f], [0x3a, 0x86, 0xff], [0x2e, 0xc4, 0xb6]];
    for (id, r, scale) in &o.tiles {
        check(*scale < 1.0, || format!("window {id} not scaled: {scale}"))?;
        let i = (0..3)
            .find(|&i| ferncomp::wm::win_id(&h.windows[i]) == Some(*id))
            .ok_or("unknown tile")?;
        let p = img.pixel(r.x + r.w / 2, r.y + r.h * 3 / 4);
        check(near(p, colours[i], 6), || {
            format!("tile {i} at {r:?} shows {p:?}")
        })?;
    }
    let dim = img.pixel(4, SIZE.1 - 4);
    check(luma(dim) < 110.0, || {
        format!("the desktop is not dimmed: {dim:?}")
    })?;
    let t = o.thumbs[0];
    let ring = img.pixel(t.x - 2, t.y + t.h / 2);
    check(near(ring, [0x7c, 0xc4, 0x8a], 12), || {
        format!("the current Space has no ring: {ring:?}")
    })?;
    // A click on the Blue tile: Overview goes, everything springs home,
    // Blue is in front.
    let (_, r, _) = o
        .tiles
        .iter()
        .find(|t| Some(t.0) == ferncomp::wm::win_id(&h.windows[1]))
        .copied()
        .ok_or("no Blue tile")?;
    h.data.state.pointer_to(
        (f64::from(r.x + r.w / 2), f64::from(r.y + r.h / 2)).into(),
        0,
    );
    h.data.state.pointer_button(0x110, true, 1);
    h.data.state.pointer_button(0x110, false, 2);
    h.settle()?;
    check(h.data.state.wm.overlay.is_empty(), || {
        "the overlay stayed".into()
    })?;
    let now: Vec<(i32, i32)> = (0..3).map(|i| h.location(i)).collect();
    check(now == homes, || format!("not home: {now:?} vs {homes:?}"))?;
    check(h.focused(1), || "the clicked window is not in front".into())
}

/// Overview as the core sees it (the window manager runs on its own
/// thread): each window's placement is its tile, the overlay's wallpaper
/// rectangles are the Spaces.
fn wm_overview_state(h: &Harness) -> Result<fernwm::policy::Overview> {
    let mut tiles = Vec::new();
    for w in &h.windows {
        let id = ferncomp::wm::win_id(w).ok_or("no id")?;
        let rec = h.data.state.wm.record(w).ok_or("no record")?;
        let size = w.geometry().size;
        let tb = ferncomp::render::TITLEBAR_HEIGHT;
        #[allow(clippy::cast_possible_truncation)]
        let scaled = |v: i32| (f64::from(v) * rec.place.scale).round() as i32;
        tiles.push((
            id,
            fernwm::policy::Rect::new(
                rec.place.x,
                rec.place.y,
                scaled(size.w),
                scaled(size.h + tb),
            ),
            rec.place.scale,
        ));
    }
    let thumbs = h
        .data
        .state
        .wm
        .overlay
        .iter()
        .filter_map(|o| match o {
            ferncomp::wm::Overlay::Wallpaper { x, y, w, h, .. } => {
                Some(fernwm::policy::Rect::new(*x, *y, *w, *h))
            }
            _ => None,
        })
        .collect();
    Ok(fernwm::policy::Overview {
        tiles,
        thumbs,
        ..fernwm::policy::Overview::default()
    })
}

fn wm_switcher(h: &mut Harness) -> Result {
    use smithay::input::keyboard::Keycode;
    // evdev codes + 8: Super (left Meta) and Tab.
    let meta = Keycode::new(125 + 8);
    let tab = Keycode::new(15 + 8);
    h.open("--color ff5a5fff --size 300x200 --title Red")?;
    h.open("--color 3a86ffff --size 300x200 --title Blue --app-id org.fern.Other")?;
    check(h.focused(1), || "Blue should be in front".into())?;
    // Hold Super, tap Tab: the switcher shows both apps, the other one
    // picked; Super still held, nothing has switched yet.
    h.data.state.key_input(meta, true, 0);
    h.data.state.key_input(tab, true, 1);
    h.data.state.key_input(tab, false, 2);
    h.settle()?;
    let img = h.shot("switcher")?;
    let panel = img.pixel(SIZE.0 / 2, SIZE.1 / 2 - 60);
    check(luma(panel) < 60.0, || {
        format!("no switcher panel: {panel:?}")
    })?;
    check(h.focused(1), || "switched before Super was let go".into())?;
    // Let go of Super: the picked app comes forward.
    h.data.state.key_input(meta, false, 3);
    h.settle()?;
    check(h.data.state.wm.overlay.is_empty(), || {
        "the switcher stayed".into()
    })?;
    check(h.focused(0), || "the picked app is not in front".into())
}

/// Blur under a window the window manager placed (and animated in), with
/// our titlebar on it.
fn wm_blur(h: &mut Harness) -> Result {
    h.open("--stripes --size 700x500")?;
    key(h, 8, 0xff52)?;
    h.open("--color ffffff00 --size 400x200 --blur")?;
    let img = h.shot("glass")?;
    let g = rect(h, 1);
    let under = spread(&img, g.y + g.h / 2, g.x + 40, g.x + g.w - 40);
    check(under < 60, || {
        format!("not blurred under the glass at {g:?}: spread {under}")
    })?;
    let beside = spread(&img, SIZE.1 - 40, 40, SIZE.0 - 40);
    check(beside > 150, || format!("stripes lost contrast: {beside}"))?;
    // Against the screen's top-left corner too (the blur reaches past
    // the edge).
    h.place(1, 0, ferncomp::render::TITLEBAR_HEIGHT)?;
    let img = h.shot("glass-corner")?;
    let g = rect(h, 1);
    let under = spread(&img, g.y + g.h / 2, g.x + 40, g.x + g.w - 40);
    check(under < 60, || {
        format!("not blurred in the corner at {g:?}: spread {under}")
    })
}

/// The shell's parts: layer surfaces for trusted clients only, the work
/// area they leave, a rounded blur behind the dock, clicks and the
/// keyboard, and global shortcuts.
#[allow(clippy::too_many_lines)]
fn wm_shell_layers(h: &mut Harness) -> Result {
    use std::io::{BufRead, BufReader};
    // An app sees neither layer-shell nor fern_shell_v1.
    let mut lister = Command::new(env!("CARGO_BIN_EXE_fern-testclient"))
        .arg("--globals")
        .env("WAYLAND_DISPLAY", &h.data.state.socket_name)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    while lister.try_wait().map_err(|e| e.to_string())?.is_none() {
        h.turn()?;
    }
    let mut globals = String::new();
    if let Some(mut out) = lister.stdout.take() {
        use std::io::Read;
        out.read_to_string(&mut globals)
            .map_err(|e| e.to_string())?;
    }
    check(globals.contains("wl_compositor"), || {
        format!("no globals listed: {globals}")
    })?;
    check(
        !globals.contains("zwlr_layer_shell_v1") && !globals.contains("fern_shell_v1"),
        || format!("an app sees the shell's protocols: {globals}"),
    )?;

    // A window tiled to fill (Super+Up): the whole screen, less the
    // gaps, while there is no shell.
    h.open("--stripes --size 300x200 --title Behind")?;
    h.data.state.focus(Some(&h.windows[0].clone()));
    key(h, 8, 0xff52)?;
    let tb = ferncomp::render::TITLEBAR_HEIGHT;
    let gap = 8;
    let filled = |top: i32, bottom: i32| {
        fernwm::policy::Rect::new(
            gap,
            top + gap + tb,
            SIZE.0 - 2 * gap,
            SIZE.1 - top - bottom - 2 * gap - tb,
        )
    };
    check(rect(h, 0) == filled(0, 0), || {
        format!("filled without a shell: {:?}", rect(h, 0))
    })?;

    // A menu bar along the top and a dock at the bottom keep their room.
    h.start_trusted(
        "--layer top --anchor top,left,right --size 10x28 --exclusive 28 --color 202020ff",
    )?;
    h.start_trusted("--layer top --anchor bottom --size 400x80 --exclusive 90 --color ffffff30 --shell-blur 0,0,400,80,20")?;
    wait_until(h, "the shell's surfaces did not appear", |h| {
        h.layers() == 2
    })?;
    h.settle()?;
    let work = h.data.state.shell.work_areas.get(&1).copied();
    check(
        work == Some(smithay::utils::Rectangle::new(
            (0, 28).into(),
            (SIZE.0, SIZE.1 - 28 - 90).into(),
        )),
        || format!("work area {work:?}"),
    )?;
    // The tiled window follows: below the bar, above the dock.
    wait_until(h, "the tiled window did not refit", |h| {
        rect(h, 0) == filled(28, 90)
    })?;
    let img = h.shot("shell")?;
    let p = img.pixel(500, 10);
    check(near(p, [0x20, 0x20, 0x20], 2), || {
        format!("menu bar: {p:?}")
    })?;
    // The dock: the stripes under it are blurred inside its rounded
    // rectangle; its bottom corner (outside the radius) is not.
    let (dx, dy) = (300, SIZE.1 - 80);
    let inside = spread(&img, dy + 40, dx + 100, dx + 300);
    let window_line = spread(&img, 300, 300, 500);
    check(inside < window_line / 2, || {
        format!("dock not blurred: spread {inside} vs {window_line} on the window")
    })?;
    let corner = img.pixel(dx + 1, dy + 78);
    let corner_blur = img.pixel(dx + 200, dy + 78);
    check(corner != corner_blur, || {
        "the dock's blur is not rounded".into()
    })?;

    // A click on the bar goes to the bar, not the window manager: the
    // window keeps its focus and nothing under the bar is hit.
    check(h.focused(0), || {
        "the window lost focus before the click".into()
    })?;
    h.data.state.pointer_to((500.0, 10.0).into(), 0);
    check(h.data.state.hit((500.0, 10.0).into()).is_none(), || {
        "the window is hit through the bar".into()
    })?;
    h.data.state.pointer_button(0x110, true, 0);
    h.data.state.pointer_button(0x110, false, 0);
    h.settle()?;
    check(h.focused(0), || {
        "a click on the bar took the window's focus".into()
    })?;

    // A panel that wants the keyboard gets it, and gives it back.
    let panel = h.start_trusted_piped(
        "--layer overlay --anchor top --size 300x200 --keyboard exclusive --color 3a86ffff",
    )?;
    wait_until(h, "the panel did not appear", |h| h.layers() == 3)?;
    h.settle()?;
    check(!h.focused(0), || {
        "the exclusive panel did not take the keyboard".into()
    })?;
    // Focusing the window meanwhile does not take the keyboard from it.
    h.data.state.focus(Some(&h.windows[0].clone()));
    check(!h.focused(0), || {
        "the window took the keyboard from the panel".into()
    })?;
    let n = h.clients.len();
    let _ = h.clients[n - 1].kill();
    drop(panel);
    wait_until(h, "the panel did not go", |h| h.layers() == 2)?;
    h.settle()?;
    check(h.focused(0), || {
        "the keyboard did not come back to the window".into()
    })?;

    // A global shortcut (Super+Space) goes to the shell, not the app.
    let out = h.start_trusted_piped(
        "--layer top --anchor top,right --size 40x28 --color 00000000 --bind 8,32",
    )?;
    wait_until(h, "the shortcut holder did not appear", |h| h.layers() == 3)?;
    h.settle()?;
    key(h, 8, 0x20)?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out)
            .lines()
            .map_while(std::result::Result::ok)
        {
            let _ = tx.send(line);
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.turn()?;
        if let Ok(line) = rx.try_recv() {
            check(line == "binding 1", || {
                format!("the shell printed {line:?}")
            })?;
            break;
        }
        check(Instant::now() < deadline, || {
            "the shortcut never reached the shell".into()
        })?;
    }
    Ok(())
}

/// The real shell (`FERN_SHELL_BIN`, built by `just shell-build`) on this
/// compositor, with its own session bus and settings daemon: the menu bar
/// and dock appear and keep their room, a running app shows in the dock,
/// Super+Space opens Search, and a notification shows as a banner.
/// Skipped (with a note) when the shell has not been built.
#[allow(clippy::too_many_lines)]
fn wm_shell_desktop(h: &mut Harness) -> Result {
    let Some(shell) = std::env::var_os("FERN_SHELL_BIN").map(PathBuf::from) else {
        println!("    skipped: FERN_SHELL_BIN is not set (just shell-test sets it)");
        return Ok(());
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("shell-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (data, config) = (tmp.join("data"), tmp.join("config"));
    let apps = data.join("applications");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config.join("fern")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(tmp.join("home")).map_err(|e| e.to_string())?;
    let client = env!("CARGO_BIN_EXE_fern-testclient");
    for (id, name, color) in [
        ("org.fern.Notes", "Notes", "ff5a5fff"),
        ("org.fern.Mail", "Mail", "3a86ffff"),
        ("org.fern.Photos", "Photos", "2ec4b6ff"),
    ] {
        std::fs::write(
            apps.join(format!("{id}.desktop")),
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={client} --title {name} --app-id {id} --color {color} --size 520x340\n"),
        )
        .map_err(|e| e.to_string())?;
    }
    // An app for text files: it shows the file it opened as its title.
    std::fs::write(
        apps.join("org.fern.Texts.desktop"),
        format!("[Desktop Entry]\nType=Application\nName=Texts\nMimeType=text/plain;\nExec={client} --title %f --app-id org.fern.Texts --size 420x260\n"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        config.join("fern/dock.toml"),
        "pinned = [\"org.fern.Notes\", \"org.fern.Mail\", \"org.fern.Photos\"]\n",
    )
    .map_err(|e| e.to_string())?;

    // A private session bus, and the user's settings daemon on it.
    let bus_conf = tmp.join("bus.conf");
    std::fs::write(
        &bus_conf,
        format!(
            "<busconfig><type>session</type><listen>unix:path={}/bus</listen><policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
            tmp.display()
        ),
    )
    .map_err(|e| e.to_string())?;
    let mut bus = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", bus_conf.display()))
        .args(["--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("dbus-daemon: {e}"))?;
    let mut address = String::new();
    {
        use std::io::BufRead;
        std::io::BufReader::new(bus.stdout.take().ok_or("no bus address")?)
            .read_line(&mut address)
            .map_err(|e| e.to_string())?;
    }
    let address = address.trim().to_owned();
    h.clients.push(bus);
    let settingsd = root.join("target/debug/fern-settingsd");
    let daemon = Command::new(&settingsd)
        .args(["--address", &address, "--dir"])
        .arg(config.join("fern"))
        .spawn()
        .map_err(|e| {
            format!(
                "{} (cargo build -p fern-settingsd): {e}",
                settingsd.display()
            )
        })?;
    h.clients.push(daemon);
    // The file indexer, over a home folder with one document in it.
    let documents = tmp.join("home/Documents");
    std::fs::create_dir_all(&documents).map_err(|e| e.to_string())?;
    std::fs::write(documents.join("Trip notes.txt"), "pack the tent").map_err(|e| e.to_string())?;
    let indexd = root.join("target/debug/fern-indexd");
    let daemon = Command::new(&indexd)
        .args(["--address", &address, "--root"])
        .arg(tmp.join("home"))
        .arg("--db")
        .arg(tmp.join("index.sqlite"))
        .spawn()
        .map_err(|e| format!("{} (cargo build -p fern-indexd): {e}", indexd.display()))?;
    h.clients.push(daemon);

    let env: Vec<(String, std::ffi::OsString)> = vec![
        ("DBUS_SESSION_BUS_ADDRESS".into(), address.clone().into()),
        ("HOME".into(), tmp.join("home").into()),
        ("XDG_DATA_HOME".into(), data.clone().into()),
        ("XDG_DATA_DIRS".into(), data.clone().into()),
        ("XDG_CONFIG_HOME".into(), config.clone().into()),
        ("FERN_FONTS".into(), root.join("build/fonts").into()),
        ("QT_QPA_PLATFORM".into(), "wayland".into()),
        // Qt's software renderer unless FERN_SHELL_QUICK_BACKEND says
        // otherwise (empty: Qt's default, OpenGL, as on the image).
        (
            "QT_QUICK_BACKEND".into(),
            std::env::var("FERN_SHELL_QUICK_BACKEND")
                .unwrap_or_else(|_| "software".into())
                .into(),
        ),
        ("QT_FORCE_STDERR_LOGGING".into(), "1".into()),
        (
            "QML_IMPORT_PATH".into(),
            format!(
                "{}:{}",
                shell
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.join("qml").display().to_string())
                    .unwrap_or_default(),
                std::env::var("QML_IMPORT_PATH").unwrap_or_default()
            )
            .into(),
        ),
    ];
    let socket = h.data.state.socket_name.clone();
    h.data.state.set_shell_launcher(Box::new(move || {
        let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
        let mut cmd = Command::new(&shell);
        cmd.stdin(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
            .env("WAYLAND_SOCKET", "0")
            .env("WAYLAND_DISPLAY", &socket);
        for (k, v) in &env {
            cmd.env(k, v);
        }
        cmd.spawn()?;
        Ok(ours)
    }))?;

    // The menu bar and the dock, and the room they keep.
    wait_long(h, "the menu bar and dock did not appear", |h| {
        h.layers() >= 2
    })?;
    wait_long(h, "the work area did not shrink", |h| {
        h.data
            .state
            .shell
            .work_areas
            .get(&1)
            .is_some_and(|a| a.loc.y >= 28 && a.size.h < SIZE.1 - 28 - 40)
    })?;
    settle_for(h, Duration::from_secs(3))?;
    let img = h.shot("desktop")?;
    // An idle desktop draws nothing: no frame in 5 s but (at most) the
    // clock's minute turning over.
    let until = Instant::now() + Duration::from_secs(5);
    let mut frames = 0;
    while Instant::now() < until {
        if h.turn()? {
            frames += 1;
        }
    }
    println!("    idle desktop: {frames} frames with damage in 5 s");
    check(frames <= 2, || {
        format!("an idle desktop drew {frames} frames in 5 s")
    })?;
    let bar = img.pixel(SIZE.0 / 2, 14);
    check(!near(bar, GREY, 6), || {
        format!("no menu bar at the top: {bar:?}")
    })?;

    // An app from its dock icon's desktop entry: its window, and the dock
    // marks it running (the shell's window list follows).
    h.open("--title Notes --app-id org.fern.Notes --color ff5a5fff --size 520x340")?;
    settle_for(h, Duration::from_secs(2))?;
    h.shot("app")?;

    // Super+Space: Search, on the overlay layer, with the keyboard.
    key(h, 8, 0x20)?;
    wait_long(h, "Search did not open", |h| h.layers() >= 3)?;
    settle_for(h, Duration::from_secs(2))?;
    h.shot("search")?;
    check(!h.focused(0), || "Search did not take the keyboard".into())?;
    // Typing "mail" finds Mail; Return opens it from its desktop entry.
    for k in ['m', 'a', 'i', 'l'] {
        key_press(h, k as u32)?;
    }
    settle_for(h, Duration::from_secs(2))?;
    h.shot("search-mail")?;
    key_press(h, 0xff0d)?;
    wait_long(h, "Search did not close", |h| !has_layer(h, "fern-popups"))?;
    h.wait_for_window("Mail, from Search")?;
    check(
        h.windows[1].toplevel().is_some_and(|t| {
            smithay::wayland::compositor::with_states(t.wl_surface(), |st| {
                st.data_map
                    .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                    .and_then(|d| d.lock().ok().and_then(|d| d.app_id.clone()))
            }) == Some("org.fern.Mail".into())
        }),
        || "Search did not open Mail".into(),
    )?;

    // The Fern menu and the control center, from the menu bar.
    click_at(h, 26.0, 14.0)?;
    wait_long(h, "the Fern menu did not open", |h| {
        has_layer(h, "fern-popups")
    })?;
    settle_for(h, Duration::from_secs(1))?;
    h.shot("fern-menu")?;
    key_press(h, 0xff1b)?;
    wait_long(h, "the menu did not close", |h| {
        !has_layer(h, "fern-popups")
    })?;
    click_at(h, f64::from(SIZE.0) - 209.0, 14.0)?;
    wait_long(h, "the control center did not open", |h| {
        has_layer(h, "fern-popups")
    })?;
    settle_for(h, Duration::from_secs(1))?;
    h.shot("control-center")?;
    key_press(h, 0xff1b)?;
    wait_long(h, "the control center did not close", |h| {
        !has_layer(h, "fern-popups")
    })?;

    // A notification: a banner at the top right.
    let conn = zbus::blocking::connection::Builder::address(address.as_str())
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    let hints: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> =
        std::collections::HashMap::new();
    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Mail",
                0u32,
                "",
                "Lunch on Friday?",
                "Sam: the place by the river at noon.",
                Vec::<&str>::new(),
                hints,
                -1i32,
            ),
        )
        .map_err(|e| format!("Notify: {e}"))?;
    let id: u32 = reply.body().deserialize().map_err(|e| e.to_string())?;
    check(id > 0, || "no notification id".into())?;
    wait_long(h, "no banner appeared", |h| h.layers() >= 3)?;
    settle_for(h, Duration::from_secs(2))?;
    let img = h.shot("banner")?;
    let corner = img.pixel(SIZE.0 - 180, 70);
    check(!near(corner, GREY, 6), || {
        let map = smithay::desktop::layer_map_for_output(h.headless.output());
        let layers: Vec<String> = map
            .layers()
            .map(|l| format!("{} {:?}", l.namespace(), map.layer_geometry(l)))
            .collect();
        format!("no banner at the top right: {corner:?}; layers {layers:?}")
    })?;

    // An app's own menus in the menu bar (the global menu): a window says
    // where they are (org_kde_kwin_appmenu); from the Fern menu, Right
    // twice reaches the app's first menu, and choosing in it reaches the
    // app.
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let saw = |what: &str| seen.lock().is_ok_and(|s| s.iter().any(|x| x == what));
    conn.object_server()
        .at("/Menu", fake::FakeMenu(seen.clone()))
        .map_err(|e| e.to_string())?;
    conn.object_server()
        .at("/StatusNotifierItem", fake::FakeItem(seen.clone()))
        .map_err(|e| e.to_string())?;
    conn.request_name("org.fern.TestApp")
        .map_err(|e| e.to_string())?;
    h.open("--title Editor --app-id org.fern.Editor --color f4f1faff --size 520x340 --appmenu org.fern.TestApp,/Menu")?;
    wait_long(h, "the shell did not read the app's menus", |_| {
        saw("GetLayout")
    })?;
    settle_for(h, Duration::from_secs(1))?;
    h.shot("app-menus")?;
    click_at(h, 26.0, 14.0)?;
    wait_long(h, "the Fern menu did not open", |h| {
        has_layer(h, "fern-popups")
    })?;
    settle_for(h, Duration::from_millis(500))?;
    key_press(h, 0xff53)?;
    key_press(h, 0xff53)?;
    wait_long(h, "Right did not reach the app's first menu", |_| {
        saw("AboutToShow 1")
    })?;
    settle_for(h, Duration::from_secs(1))?;
    h.shot("app-menu-open")?;
    key_press(h, 0xff54)?;
    key_press(h, 0xff0d)?;
    wait_long(h, "choosing Open did not reach the app", |_| {
        saw("clicked 2")
    })?;
    wait_long(h, "the app's menu did not close", |h| {
        !has_layer(h, "fern-popups")
    })?;

    // An app's status icon (StatusNotifierItem): it registers with the
    // shell, shows in the menu bar (its magenta pixmap), and a click
    // reaches the app.
    conn.call_method(
        Some("org.kde.StatusNotifierWatcher"),
        "/StatusNotifierWatcher",
        Some("org.kde.StatusNotifierWatcher"),
        "RegisterStatusNotifierItem",
        &("/StatusNotifierItem",),
    )
    .map_err(|e| format!("RegisterStatusNotifierItem: {e}"))?;
    settle_for(h, Duration::from_secs(2))?;
    let img = h.shot("tray")?;
    let icon = (0..SIZE.0)
        .find(|&x| near(img.pixel(x, 14), [255, 0, 255], 40))
        .ok_or("no status icon in the menu bar")?;
    click_at(h, f64::from(icon), 14.0)?;
    wait_long(
        h,
        "a click on the status icon did not reach the app",
        |_| saw("Activate"),
    )?;

    // Night Shift, turned on in the settings: the shell asks the
    // compositor for warm colours (warmth 60: 4020 K), and for neutral
    // ones when it is turned off.
    let setting = |domain: &str, key: &str, value: Value<'_>| {
        conn.call_method(
            Some("org.fern.Settings"),
            "/org/fern/Settings",
            Some("org.fern.Settings1"),
            "Set",
            &(domain, key, value),
        )
        .map(|_| ())
        .map_err(|e| format!("Set {domain}.{key}: {e}"))
    };
    let set = |key: &str, value: Value<'_>| setting("nightshift", key, value);
    set("manual", true.into())?;
    wait_long(h, "Night Shift did not warm the colours", |h| {
        h.data.state.color_temperature == 4020
    })?;
    set("manual", false.into())?;
    wait_long(h, "Night Shift did not end", |h| {
        h.data.state.color_temperature == 0
    })?;

    // Screenshots, saved by the shell in ~/Pictures/Screenshots: the
    // whole display (Shift+Super+3), then a dragged rectangle
    // (Shift+Super+4).
    let shots = tmp.join("home/Pictures/Screenshots");
    let saved = |n: usize| {
        let shots = shots.clone();
        move |_: &Harness| std::fs::read_dir(&shots).is_ok_and(|d| d.count() >= n)
    };
    key(h, 9, 0x33)?;
    wait_long(h, "no screenshot of the display was saved", saved(1))?;
    key(h, 9, 0x34)?;
    wait_long(h, "the rectangle chooser did not open", |h| {
        has_layer(h, "fern-screenshot")
    })?;
    settle_for(h, Duration::from_millis(500))?;
    h.data.state.pointer_to((100.0, 100.0).into(), 0);
    settle_for(h, Duration::from_millis(300))?;
    h.data.state.pointer_button(0x110, true, 1);
    for (x, y) in [(150.0, 140.0), (250.0, 220.0), (300.0, 250.0)] {
        h.data.state.pointer_to((x, y).into(), 2);
        settle_for(h, Duration::from_millis(100))?;
    }
    h.shot("screenshot-select")?;
    h.data.state.pointer_button(0x110, false, 3);
    wait_long(h, "no screenshot of the rectangle was saved", saved(2))?;
    let mut sizes: Vec<(u32, u32)> = std::fs::read_dir(tmp.join("home/Pictures/Screenshots"))
        .map_err(|e| e.to_string())?
        .filter_map(|e| {
            let f = std::fs::File::open(e.ok()?.path()).ok()?;
            let info = png::Decoder::new(std::io::BufReader::new(f))
                .read_info()
                .ok()?;
            Some((info.info().width, info.info().height))
        })
        .collect();
    sizes.sort_unstable();
    check(
        sizes == vec![(200, 150), (SIZE.0.cast_unsigned(), SIZE.1.cast_unsigned())],
        || format!("screenshot sizes {sizes:?}"),
    )?;

    // Dark mode with Grove at night (`just wallpapers` renders the pair):
    // the menu bar turns dark and the compositor crossfades to the night
    // picture; back to light, back to Grove by day.
    let walls = root.join("build/wallpapers");
    if walls.join("grove-night.png").is_file() {
        let path = |name: &str| Value::from(walls.join(name).display().to_string());
        setting("wallpaper", "light", path("grove.png"))?;
        setting("wallpaper", "dark", path("grove-night.png"))?;
        let sky = |h: &mut Harness| -> std::result::Result<f64, String> {
            Ok(luma(h.shot("theme")?.pixel(SIZE.0 / 2, 60)))
        };
        settle_for(h, Duration::from_secs(2))?;
        let day = sky(h)?;
        setting("appearance", "theme", "dark".into())?;
        // The fade takes 0.88 s.
        settle_for(h, Duration::from_secs(3))?;
        let img = h.shot("dark")?;
        let (night, bar) = (
            luma(img.pixel(SIZE.0 / 2, 60)),
            luma(img.pixel(SIZE.0 / 2, 14)),
        );
        check(night < 70.0 && day > 120.0, || {
            format!("the sky did not turn to night: {day:.0} by day, {night:.0} at night")
        })?;
        check(bar < 70.0, || {
            format!("the menu bar did not turn dark: {bar:.0}")
        })?;
        check(h.data.state.dark, || {
            "the titlebars did not turn dark".into()
        })?;
        setting("appearance", "theme", "light".into())?;
        settle_for(h, Duration::from_secs(3))?;
        let again = sky(h)?;
        check(again > 120.0, || {
            format!("the sky did not turn back to day: {again:.0}")
        })?;
        check(!h.data.state.dark, || "the titlebars stayed dark".into())?;
    } else {
        println!("    dark mode skipped: no build/wallpapers (just wallpapers)");
    }

    // "notification" in Search opens the Notification Center: actions
    // whose title matches come before Do Not Disturb, whose keywords match
    // too. Escape closes it.
    key(h, 8, 0x20)?;
    wait_long(h, "Search did not open for the Notification Center", |h| {
        has_layer(h, "fern-popups")
    })?;
    settle_for(h, Duration::from_millis(500))?;
    for k in "notification".chars() {
        key_press(h, k as u32)?;
    }
    settle_for(h, Duration::from_secs(2))?;
    key_press(h, 0xff0d)?;
    settle_for(h, Duration::from_secs(2))?;
    h.shot("search-notification-center")?;
    let dnd = conn
        .call_method(
            Some("org.fern.Settings"),
            "/org/fern/Settings",
            Some("org.fern.Settings1"),
            "Get",
            &("notifications", "dnd"),
        )
        .map_err(|e| format!("Get notifications.dnd: {e}"))?
        .body()
        .deserialize::<zbus::zvariant::OwnedValue>()
        .map_err(|e| format!("notifications.dnd: {e}"))
        .and_then(|v| bool::try_from(v).map_err(|e| format!("notifications.dnd: {e}")))?;
    check(!dnd, || {
        "\"notification\" in Search turned on Do Not Disturb".into()
    })?;
    check(has_layer(h, "fern-popups"), || {
        "\"notification\" in Search opened nothing".into()
    })?;
    key_press(h, 0xff1b)?;
    wait_long(h, "the Notification Center did not close", |h| {
        !has_layer(h, "fern-popups")
    })?;

    // Files in Search: "trip" finds Documents/Trip notes.txt through the
    // indexer, and Return opens it in the app for text files, which gets
    // the file's path.
    key(h, 8, 0x20)?;
    wait_long(h, "Search did not open again", |h| {
        has_layer(h, "fern-popups")
    })?;
    settle_for(h, Duration::from_millis(500))?;
    for k in ['t', 'r', 'i', 'p'] {
        key_press(h, k as u32)?;
    }
    settle_for(h, Duration::from_secs(2))?;
    h.shot("search-file")?;
    key_press(h, 0xff0d)?;
    let w = h.wait_for_window("Texts, from Search")?;
    let title = w.toplevel().and_then(|t| {
        smithay::wayland::compositor::with_states(t.wl_surface(), |st| {
            st.data_map
                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok().and_then(|d| d.title.clone()))
        })
    });
    let want = documents.join("Trip notes.txt").display().to_string();
    check(title.as_deref() == Some(want.as_str()), || {
        format!("Search did not open the file: a window titled {title:?}")
    })?;
    Ok(())
}

/// Fake apps on the session bus for the shell to talk to.
mod fake {
    // The interface macro reads every argument, unused or not, and
    // properties are methods even when they are constants.
    #![allow(clippy::used_underscore_binding, clippy::unused_self)]

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use zbus::zvariant::{OwnedValue, Value};

    /// A fake app's menus (com.canonical.dbusmenu): File with Open, a
    /// separator and a disabled Quit; and Edit. It notes what it is asked.
    pub struct FakeMenu(pub Arc<Mutex<Vec<String>>>);

    pub type MenuNode = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

    fn menu_node(id: i32, props: &[(&str, Value<'static>)], children: Vec<MenuNode>) -> MenuNode {
        let props = props
            .iter()
            .filter_map(|(k, v)| {
                Some((
                    (*k).to_owned(),
                    OwnedValue::try_from(v.try_clone().ok()?).ok()?,
                ))
            })
            .collect();
        let children = children
            .into_iter()
            .filter_map(|c| OwnedValue::try_from(Value::from(c)).ok())
            .collect();
        (id, props, children)
    }

    #[zbus::interface(name = "com.canonical.dbusmenu")]
    impl FakeMenu {
        fn get_layout(&self, _parent: i32, _depth: i32, _props: Vec<String>) -> (u32, MenuNode) {
            if let Ok(mut s) = self.0.lock() {
                s.push("GetLayout".into());
            }
            let file = menu_node(
                1,
                &[("label", "_File".into())],
                vec![
                    menu_node(2, &[("label", "Open".into())], vec![]),
                    menu_node(3, &[("type", "separator".into())], vec![]),
                    menu_node(
                        4,
                        &[("label", "Quit".into()), ("enabled", false.into())],
                        vec![],
                    ),
                ],
            );
            let edit = menu_node(
                5,
                &[("label", "Edit".into())],
                vec![menu_node(6, &[("label", "Copy".into())], vec![])],
            );
            (1, menu_node(0, &[], vec![file, edit]))
        }

        fn event(&self, id: i32, event: &str, _data: OwnedValue, _time: u32) {
            if let Ok(mut s) = self.0.lock() {
                s.push(format!("{event} {id}"));
            }
        }

        fn about_to_show(&self, id: i32) -> bool {
            if let Ok(mut s) = self.0.lock() {
                s.push(format!("AboutToShow {id}"));
            }
            false
        }
    }

    /// A fake app's status icon (org.kde.StatusNotifierItem): magenta, no
    /// menu, so a click activates it.
    pub struct FakeItem(pub Arc<Mutex<Vec<String>>>);

    #[zbus::interface(name = "org.kde.StatusNotifierItem")]
    impl FakeItem {
        fn activate(&self, _x: i32, _y: i32) {
            if let Ok(mut s) = self.0.lock() {
                s.push("Activate".into());
            }
        }
        #[zbus(property)]
        fn id(&self) -> String {
            "fake".into()
        }
        #[zbus(property)]
        fn title(&self) -> String {
            "Fake".into()
        }
        #[zbus(property)]
        fn status(&self) -> String {
            "Active".into()
        }
        #[zbus(property)]
        fn menu(&self) -> zbus::zvariant::OwnedObjectPath {
            zbus::zvariant::OwnedObjectPath::try_from("/NO_DBUSMENU").unwrap_or_default()
        }
        #[zbus(property)]
        fn item_is_menu(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
            vec![(16, 16, [0xff, 0xff, 0x00, 0xff].repeat(256))]
        }
    }
}

/// Like `wait_until`, for the shell's slower start (Qt, QML, a bus).
fn wait_long(h: &mut Harness, what: &str, cond: impl Fn(&Harness) -> bool) -> Result {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !cond(h) {
        check(Instant::now() < deadline, || format!("timed out: {what}"))?;
        h.turn()?;
    }
    Ok(())
}

/// Runs turns for a while (the shell animates in on springs).
fn settle_for(h: &mut Harness, d: Duration) -> Result {
    let until = Instant::now() + d;
    while Instant::now() < until {
        h.turn()?;
    }
    Ok(())
}

/// Whether a shell surface with this namespace is mapped.
fn has_layer(h: &Harness, namespace: &str) -> bool {
    smithay::desktop::layer_map_for_output(h.headless.output())
        .layers()
        .any(|l| l.namespace() == namespace)
}

fn click_at(h: &mut Harness, x: f64, y: f64) -> Result {
    // Moving first, as a hand does: the client sees where the pointer is
    // before the press.
    h.data.state.pointer_to((x, y).into(), 0);
    settle_for(h, Duration::from_millis(300))?;
    h.data.state.pointer_button(0x110, true, 1);
    h.data.state.pointer_button(0x110, false, 2);
    h.settle()
}

/// Presses and releases a key that goes to the focused client (US
/// layout).
fn key_press(h: &mut Harness, keysym: u32) -> Result {
    // xkb keycodes are evdev + 8. Letters a-z, in alphabetical order.
    const LETTERS: [u32; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    let evdev = match keysym {
        0x61..=0x7a => LETTERS[(keysym - 0x61) as usize],
        0xff1b => 1,   // Escape
        0xff0d => 28,  // Return
        0xff51 => 105, // Left
        0xff53 => 106, // Right
        0xff54 => 108, // Down
        _ => return Err(format!("no keycode for {keysym:#x}")),
    };
    let code = evdev + 8u32;
    h.data.state.key_input(code.into(), true, 0);
    h.data.state.key_input(code.into(), false, 1);
    h.settle()
}

const SCENARIOS: &[Scenario] = &[
    ("orientation", orientation),
    ("corners_and_shadow", corners_and_shadow),
    ("focus_shadow", focus_shadow),
    ("blur", blur),
    ("blur_region", blur_region),
    ("idle_frames_have_no_damage", idle_frames_have_no_damage),
    ("wm_places_and_focuses", wm_places_and_focuses),
    ("wm_restart_keeps_windows", wm_restart_keeps_windows),
    ("wm_titlebar", wm_titlebar),
    ("wm_zoom", wm_zoom),
    ("wm_tiling", wm_tiling),
    ("wm_spaces", wm_spaces),
    ("wm_minimize_to_dock", wm_minimize_to_dock),
    ("wm_overview", wm_overview),
    ("wm_switcher", wm_switcher),
    ("wm_blur", wm_blur),
    ("wm_shell_layers", wm_shell_layers),
    ("wm_shell_desktop", wm_shell_desktop),
];

fn main() -> ExitCode {
    let runtime = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fern-comp-runtime-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&runtime) {
        eprintln!("{}: {e}", runtime.display());
        return ExitCode::FAILURE;
    }
    // SAFETY: no other thread exists yet (this binary has no test
    // harness), so nothing reads the environment while it changes. The
    // values select Mesa's software renderer and a private socket dir.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", &runtime);
        std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        // Titles in screenshots, when `just fonts` has unpacked Inter.
        std::env::set_var(
            "FERN_FONTS",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/fonts"),
        );
        if let Some(mesa) = std::env::var_os("MESA_EGL_VENDOR_DIR") {
            std::env::set_var("__EGL_VENDOR_LIBRARY_DIRS", mesa);
        }
    }
    let filter = std::env::args().nth(1).filter(|a| !a.starts_with('-'));
    let mut failed = 0;
    let mut ran = 0;
    for (name, scenario) in SCENARIOS {
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
            continue;
        }
        ran += 1;
        // Scenarios named wm_* run with fern-wm; the rest without (the
        // core's fallback placement), so pixel checks do not depend on it.
        let harness = if name.starts_with("wm_") {
            Harness::with_wm(name)
        } else {
            Harness::new(name)
        };
        let result = harness.and_then(|mut h| scenario(&mut h));
        match result {
            Ok(()) => println!("test {name} ... ok"),
            Err(e) => {
                println!("test {name} ... FAILED: {e}");
                failed += 1;
            }
        }
    }
    let _ = std::fs::remove_dir(&runtime);
    println!("render: {} passed, {failed} failed", ran - failed);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
