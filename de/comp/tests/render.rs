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
use std::time::{Duration, Instant};

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

const SCENARIOS: &[Scenario] = &[
    ("orientation", orientation),
    ("corners_and_shadow", corners_and_shadow),
    ("focus_shadow", focus_shadow),
    ("blur", blur),
    ("blur_region", blur_region),
    ("idle_frames_have_no_damage", idle_frames_have_no_damage),
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
        let result = Harness::new(name).and_then(|mut h| scenario(&mut h));
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
