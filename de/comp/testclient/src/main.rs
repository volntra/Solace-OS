//! sol-testclient: a small, scriptable Wayland client for testing
//! sol-comp. It opens one window filled with a colour or a test pattern,
//! and can ask for background blur.
//!
//! ```text
//! sol-testclient [--size WxH] [--color RRGGBBAA] [--stripes] [--blur] [--title T]
//! ```

use std::io::Write;
use std::os::fd::AsFd;

use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_registry::WlRegistry,
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};
use wayland_protocols_plasma::blur::client::{
    org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
};

/// What to draw.
#[derive(Debug, Clone, PartialEq)]
struct Options {
    size: (i32, i32),
    /// Straight (not premultiplied) RGBA.
    color: [u8; 4],
    stripes: bool,
    blur: bool,
    title: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            size: (480, 320),
            color: [0xf4, 0xf1, 0xfa, 0xff],
            stripes: false,
            blur: false,
            title: "Test".into(),
        }
    }
}

fn parse_size(s: &str) -> Option<(i32, i32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0 && w <= 8192 && h <= 8192).then_some((w, h))
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
            "--blur" => o.blur = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(o)
}

/// The window's pixels: little-endian ARGB8888 (B, G, R, A bytes),
/// premultiplied.
fn pixels(o: &Options, (w, h): (i32, i32)) -> Vec<u8> {
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
                bands[usize::try_from((x + y) / 24).unwrap_or(0) % bands.len()]
            } else {
                o.color
            };
            out.extend_from_slice(&premultiply(c));
        }
    }
    out
}

struct App {
    options: Options,
    shm: WlShm,
    surface: WlSurface,
    size: (i32, i32),
    /// Set by a configure: draw at this size before the next commit.
    redraw: bool,
    closed: bool,
}

impl App {
    fn draw(&mut self, qh: &QueueHandle<App>) -> Result<(), String> {
        let (w, h) = self.size;
        let data = pixels(&self.options, self.size);
        let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
        let path =
            std::path::Path::new(&dir).join(format!("sol-testclient-{}", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let _ = std::fs::remove_file(&path);
        file.write_all(&data).map_err(|e| format!("shm: {e}"))?;
        let len = i32::try_from(data.len()).map_err(|_| "window too large")?;
        let pool = self.shm.create_pool(file.as_fd(), len, qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface.damage_buffer(0, 0, w, h);
        self.surface.commit();
        Ok(())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("sol-testclient: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(options) {
        eprintln!("sol-testclient: {e}");
        std::process::exit(1);
    }
}

fn run(options: Options) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
    let (globals, mut queue) =
        registry_queue_init::<App>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
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
    toplevel.set_app_id("org.sol.TestClient".into());
    if options.blur {
        let manager: OrgKdeKwinBlurManager = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("blur protocol: {e}"))?;
        let blur = manager.create(&surface, &qh, ());
        // No region: the whole surface.
        blur.commit();
    }
    surface.commit();

    let mut app = App {
        size: options.size,
        options,
        shm,
        surface,
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
            xdg_toplevel::Event::Configure { width, height, .. } if width > 0 && height > 0 => {
                app.size = (width, height);
            }
            xdg_toplevel::Event::Close => app.closed = true,
            _ => {}
        }
    }
}

delegate_noop!(App: WlCompositor);
delegate_noop!(App: WlShmPool);
delegate_noop!(App: OrgKdeKwinBlurManager);
delegate_noop!(App: OrgKdeKwinBlur);
delegate_noop!(App: ignore WlShm);
delegate_noop!(App: ignore WlSurface);
delegate_noop!(App: ignore WlBuffer);

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
        let px = pixels(&o, (2, 1));
        assert_eq!(px.len(), 8);
        assert_eq!(&px[..4], &[25, 50, 100, 128]);
    }
}
