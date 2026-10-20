//! fern-testclient: a small, scriptable Wayland client for testing
//! fern-comp. It opens one window filled with a colour or a test pattern,
//! and can ask for background blur.
//!
//! ```text
//! fern-testclient [--size WxH] [--color RRGGBBAA] [--stripes] [--animate]
//!                [--blur | --blur-region X,Y,W,H] [--title T] [--csd] [--globals]
//!                [--dock TITLE]
//! ```
//!
//! Like most apps it asks for server-side decoration (our titlebar) when
//! the compositor offers xdg-decoration; `--csd` does not ask.
//!
//! `--animate` moves the stripes every frame the compositor asks for, so
//! it draws at the display's refresh rate (for frame timing). `--globals`
//! lists the compositor's globals and exits.
//!
//! `--dock TITLE` plays a dock over wlr-foreign-toplevel (only for a
//! trusted connection, `WAYLAND_SOCKET`): it says the window titled TITLE
//! has its icon at 10,10 48x48 on our surface and minimizes it; half a
//! second after that window reports minimized, it activates it again.

use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;

use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::{self, WlBuffer},
        wl_callback::{self, WlCallback},
        wl_compositor::WlCompositor,
        wl_region::WlRegion,
        wl_registry::WlRegistry,
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
};
use wayland_protocols::xdg::decoration::zv1::client::{
    zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
    zxdg_toplevel_decoration_v1::{self, ZxdgToplevelDecorationV1},
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};
use wayland_protocols_plasma::blur::client::{
    org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

/// What to draw.
#[derive(Debug, Clone, PartialEq)]
// Command-line flags: each bool is one switch.
#[allow(clippy::struct_excessive_bools)]
struct Options {
    size: (i32, i32),
    /// Straight (not premultiplied) RGBA.
    color: [u8; 4],
    stripes: bool,
    /// New frame on every frame callback (implies `stripes`).
    animate: bool,
    /// Print the globals (interface and version) and exit.
    globals: bool,
    blur: bool,
    /// Blur only this part of the surface (implies `blur`).
    blur_region: Option<(i32, i32, i32, i32)>,
    title: String,
    /// Do not ask for server-side decoration.
    csd: bool,
    /// Play a dock for the window with this title.
    dock: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            size: (480, 320),
            color: [0xf4, 0xf1, 0xfa, 0xff],
            stripes: false,
            animate: false,
            globals: false,
            blur: false,
            blur_region: None,
            title: "Test".into(),
            csd: false,
            dock: None,
        }
    }
}

fn parse_size(s: &str) -> Option<(i32, i32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0 && w <= 8192 && h <= 8192).then_some((w, h))
}

fn parse_rect(text: &str) -> Option<(i32, i32, i32, i32)> {
    let v: Vec<i32> = text
        .split(',')
        .map(|n| n.parse().ok())
        .collect::<Option<_>>()?;
    match v[..] {
        [x, y, width, height] if width > 0 && height > 0 => Some((x, y, width, height)),
        _ => None,
    }
}

fn parse_color(s: &str) -> Option<[u8; 4]> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 8 || !s.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?])
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--size" => o.size = parse_size(value()?).ok_or("--size wants WxH")?,
            "--color" => o.color = parse_color(value()?).ok_or("--color wants RRGGBBAA")?,
            "--title" => o.title.clone_from(value()?),
            "--stripes" => o.stripes = true,
            "--csd" => o.csd = true,
            "--dock" => o.dock = Some(value()?.clone()),
            "--globals" => o.globals = true,
            "--animate" => {
                o.animate = true;
                o.stripes = true;
            }
            "--blur" => o.blur = true,
            "--blur-region" => {
                o.blur_region = Some(parse_rect(value()?).ok_or("--blur-region wants X,Y,W,H")?);
                o.blur = true;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(o)
}

/// The window's pixels: little-endian ARGB8888 (B, G, R, A bytes),
/// premultiplied. `phase` shifts the stripes (animation).
fn pixels(o: &Options, (w, h): (i32, i32), phase: i32) -> Vec<u8> {
    let premultiply = |[r, g, b, a]: [u8; 4]| {
        let m = |c: u8| u8::try_from(u16::from(c) * u16::from(a) / 255).unwrap_or(255);
        [m(b), m(g), m(r), a]
    };
    // High-contrast diagonal bands, to make blur visible behind others.
    let bands = [
        [0xff, 0x5a, 0x5f, 0xff],
        [0xff, 0xd1, 0x66, 0xff],
        [0x2e, 0xc4, 0xb6, 0xff],
        [0x3a, 0x86, 0xff, 0xff],
    ];
    let mut out = Vec::with_capacity(usize::try_from(w * h * 4).unwrap_or(0));
    for y in 0..h {
        for x in 0..w {
            let c = if o.stripes {
                bands[usize::try_from((x + y + phase).rem_euclid(96) / 24).unwrap_or(0)]
            } else {
                o.color
            };
            out.extend_from_slice(&premultiply(c));
        }
    }
    out
}

/// Two shm buffers in one file, used in turn: one can be on screen while
/// the other is drawn.
struct Buffers {
    file: std::fs::File,
    size: (i32, i32),
    generation: u32,
    slots: [WlBuffer; 2],
    busy: [bool; 2],
}

struct App {
    options: Options,
    shm: WlShm,
    surface: WlSurface,
    size: (i32, i32),
    buffers: Option<Buffers>,
    generation: u32,
    /// Stripe offset, advanced per frame when animating.
    phase: i32,
    /// Set by a configure or a frame callback: draw before the next commit.
    redraw: bool,
    closed: bool,
    dock: Option<Dock>,
}

/// `--dock`: the target window's handle and how far we got with it.
struct Dock {
    target: String,
    seat: WlSeat,
    /// Every window's handle and its title so far.
    titles: Vec<(ZwlrForeignToplevelHandleV1, String)>,
    /// Our own window is on screen (`set_rectangle` is relative to it): the
    /// window manager activates a window as it shows it.
    on_screen: bool,
    minimized: bool,
    activated: bool,
}

impl App {
    fn buffers(&mut self, qh: &QueueHandle<App>) -> Result<&mut Buffers, String> {
        if self.buffers.as_ref().is_none_or(|b| b.size != self.size) {
            let (w, h) = self.size;
            let frame = w * h * 4;
            let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
            let path =
                std::path::Path::new(&dir).join(format!("fern-testclient-{}", std::process::id()));
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let _ = std::fs::remove_file(&path);
            file.set_len(u64::try_from(frame * 2).map_err(|_| "window too large")?)
                .map_err(|e| format!("shm: {e}"))?;
            let pool = self.shm.create_pool(file.as_fd(), frame * 2, qh, ());
            self.generation += 1;
            let g = self.generation;
            let slot = |i: usize, offset: i32| {
                pool.create_buffer(offset, w, h, w * 4, wl_shm::Format::Argb8888, qh, (g, i))
            };
            let slots = [slot(0, 0), slot(1, frame)];
            pool.destroy();
            if let Some(old) = self.buffers.take() {
                for b in old.slots {
                    b.destroy();
                }
            }
            self.buffers = Some(Buffers {
                file,
                size: self.size,
                generation: g,
                slots,
                busy: [false; 2],
            });
        }
        self.buffers
            .as_mut()
            .ok_or_else(|| "no buffers".to_string())
    }

    fn draw(&mut self, qh: &QueueHandle<App>) -> Result<(), String> {
        let data = pixels(&self.options, self.size, self.phase);
        let (w, h) = self.size;
        let animate = self.options.animate;
        let bufs = self.buffers(qh)?;
        // Both buffers still in use: draw when one is released.
        let Some(i) = bufs.busy.iter().position(|b| !b) else {
            return Ok(());
        };
        let offset = u64::try_from(i * data.len()).map_err(|_| "window too large")?;
        bufs.file
            .write_all_at(&data, offset)
            .map_err(|e| format!("shm: {e}"))?;
        bufs.busy[i] = true;
        let buffer = bufs.slots[i].clone();
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface.damage_buffer(0, 0, w, h);
        if animate {
            self.surface.frame(qh, ());
            self.phase = (self.phase + 2) % 96;
        }
        self.surface.commit();
        Ok(())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("fern-testclient: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(options) {
        eprintln!("fern-testclient: {e}");
        std::process::exit(1);
    }
}

fn run(options: Options) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
    let (globals, mut queue) =
        registry_queue_init::<App>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
    if options.globals {
        for g in globals.contents().clone_list() {
            println!("{} {}", g.interface, g.version);
        }
        return Ok(());
    }
    let compositor: WlCompositor = globals
        .bind(&qh, 1..=6, ())
        .map_err(|e| format!("wl_compositor: {e}"))?;
    let shm: WlShm = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("wl_shm: {e}"))?;
    let wm: XdgWmBase = globals
        .bind(&qh, 1..=6, ())
        .map_err(|e| format!("xdg_wm_base: {e}"))?;
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title(options.title.clone());
    toplevel.set_app_id("org.fern.TestClient".into());
    if !options.csd
        && let Ok(manager) = globals.bind::<ZxdgDecorationManagerV1, _, _>(&qh, 1..=1, ())
    {
        let deco = manager.get_toplevel_decoration(&toplevel, &qh, ());
        deco.set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
    }
    if options.blur {
        let manager: OrgKdeKwinBlurManager = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("blur protocol: {e}"))?;
        let blur = manager.create(&surface, &qh, ());
        // No region: the whole surface.
        if let Some((x, y, w, h)) = options.blur_region {
            let region = compositor.create_region(&qh, ());
            region.add(x, y, w, h);
            blur.set_region(Some(&region));
            region.destroy();
        }
        blur.commit();
    }
    let dock = match options.dock.clone() {
        Some(target) => {
            let _: ZwlrForeignToplevelManagerV1 = globals
                .bind(&qh, 1..=3, ())
                .map_err(|e| format!("foreign toplevel manager (not trusted?): {e}"))?;
            let seat: WlSeat = globals
                .bind(&qh, 1..=1, ())
                .map_err(|e| format!("wl_seat: {e}"))?;
            Some(Dock {
                target,
                seat,
                titles: Vec::new(),
                on_screen: false,
                minimized: false,
                activated: false,
            })
        }
        None => None,
    };
    surface.commit();

    let mut app = App {
        dock,
        size: options.size,
        options,
        shm,
        surface,
        buffers: None,
        generation: 0,
        phase: 0,
        redraw: false,
        closed: false,
    };
    while !app.closed {
        queue
            .blocking_dispatch(&mut app)
            .map_err(|e| format!("dispatch: {e}"))?;
        if app.redraw {
            app.redraw = false;
            app.draw(&qh)?;
        }
        app.dock_act();
    }
    Ok(())
}

impl Dispatch<WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgDecorationManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZxdgDecorationManagerV1,
        _: <ZxdgDecorationManagerV1 as wayland_client::Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgToplevelDecorationV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZxdgToplevelDecorationV1,
        _: zxdg_toplevel_decoration_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgWmBase, ()> for App {
    fn event(
        _: &mut Self,
        wm: &XdgWmBase,
        event: xdg_wm_base::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for App {
    fn event(
        app: &mut Self,
        xdg: &XdgSurface,
        event: xdg_surface::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg.ack_configure(serial);
            app.redraw = true;
        }
    }
}

impl Dispatch<XdgToplevel, ()> for App {
    fn event(
        app: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                if width > 0 && height > 0 {
                    app.size = (width, height);
                }
                let activated = states
                    .chunks_exact(4)
                    .filter_map(|c| c.try_into().ok().map(u32::from_ne_bytes))
                    .any(|s| s == xdg_toplevel::State::Activated as u32);
                if activated && let Some(dock) = app.dock.as_mut() {
                    dock.on_screen = true;
                }
            }
            xdg_toplevel::Event::Close => app.closed = true,
            _ => {}
        }
    }
}

delegate_noop!(App: ignore WlSeat);

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event
            && let Some(dock) = app.dock.as_mut()
        {
            dock.titles.push((toplevel, String::new()));
        }
    }

    wayland_client::event_created_child!(App, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for App {
    fn event(
        app: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::{Event, State};
        let Some(dock) = app.dock.as_mut() else {
            return;
        };
        let Some(i) = dock.titles.iter().position(|(h, _)| h == handle) else {
            return;
        };
        match event {
            Event::Title { title } => dock.titles[i].1 = title,
            Event::State { state } => {
                let minimized = state
                    .chunks_exact(4)
                    .filter_map(|c| c.try_into().ok().map(u32::from_ne_bytes))
                    .any(|s| s == State::Minimized as u32);
                if dock.titles[i].1 == dock.target && minimized && dock.minimized && !dock.activated
                {
                    // Time for a test to look at the minimized window.
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    handle.activate(&dock.seat);
                    dock.activated = true;
                }
            }
            Event::Closed => {
                dock.titles.remove(i);
            }
            _ => {}
        }
    }
}

delegate_noop!(App: WlCompositor);
delegate_noop!(App: WlShmPool);
delegate_noop!(App: WlRegion);
delegate_noop!(App: OrgKdeKwinBlurManager);
delegate_noop!(App: OrgKdeKwinBlur);
delegate_noop!(App: ignore WlShm);
delegate_noop!(App: ignore WlSurface);

impl App {
    /// `--dock`: once our window is on screen and the target is known,
    /// put its icon on us and minimize it (once).
    fn dock_act(&mut self) {
        let Some(dock) = self.dock.as_mut() else {
            return;
        };
        if !dock.on_screen || dock.minimized {
            return;
        }
        if let Some((handle, _)) = dock.titles.iter().find(|(_, t)| *t == dock.target) {
            handle.set_rectangle(&self.surface, 10, 10, 48, 48);
            handle.set_minimized();
            dock.minimized = true;
        }
    }
}
impl Dispatch<WlBuffer, (u32, usize)> for App {
    fn event(
        app: &mut Self,
        _: &WlBuffer,
        event: wl_buffer::Event,
        (generation, i): &(u32, usize),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event
            && let Some(b) = app.buffers.as_mut()
            && b.generation == *generation
        {
            b.busy[*i] = false;
        }
    }
}

impl Dispatch<WlCallback, ()> for App {
    fn event(
        app: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            app.redraw = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_options() {
        let o = parse_args(&args(
            "--size 900x260 --color ffffff66 --blur --title Glass",
        ))
        .unwrap();
        assert_eq!(o.size, (900, 260));
        assert_eq!(o.color, [0xff, 0xff, 0xff, 0x66]);
        assert!(o.blur && !o.stripes);
        assert_eq!(o.title, "Glass");
        assert_eq!(parse_args(&[]).unwrap(), Options::default());
    }

    #[test]
    fn animate_implies_stripes_and_moves_them() {
        let o = parse_args(&args("--animate")).unwrap();
        assert!(o.animate && o.stripes);
        assert_ne!(pixels(&o, (8, 1), 0), pixels(&o, (8, 1), 24));
        assert_eq!(pixels(&o, (8, 1), 0), pixels(&o, (8, 1), 96));
    }

    #[test]
    fn a_blur_region_implies_blur() {
        let o = parse_args(&args("--blur-region 0,10,200,50")).unwrap();
        assert!(o.blur);
        assert_eq!(o.blur_region, Some((0, 10, 200, 50)));
    }

    #[test]
    fn rejects_bad_options() {
        for bad in [
            "--size 0x10",
            "--size 10",
            "--color fff",
            "--color zzzzzzzz",
            "--size",
            "--nope",
        ] {
            assert!(parse_args(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn pixels_are_premultiplied_bgra() {
        let o = Options {
            color: [200, 100, 50, 128],
            ..Options::default()
        };
        let px = pixels(&o, (2, 1), 0);
        assert_eq!(px.len(), 8);
        assert_eq!(&px[..4], &[25, 50, 100, 128]);
    }
}
