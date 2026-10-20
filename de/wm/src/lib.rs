//! fern-wm: Fern's window manager (CLAUDE.md 6.6). A separate process
//! that talks to the core (fern-comp) over the private `fern_wm_v1`
//! protocol (docs/protocols/wm.md). The decisions are in [`policy`]; this
//! module only carries events in and commands out.

pub mod policy;

use fern_wm_proto::client::fern_wm_v1::{self, FernWmV1};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};

use policy::{Cmd, Event, Wm};

struct App {
    wm: Wm,
    proxy: FernWmV1,
    /// Commands not yet sent (sent and committed after each batch).
    queue: Vec<Cmd>,
}

/// Runs the window manager on `conn` until the core goes away.
///
/// # Errors
/// The core does not offer `fern_wm_v1` (this process was not started by
/// it), or the connection fails.
pub fn run(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let (globals, mut queue) = registry_queue_init::<App>(conn)?;
    let qh = queue.handle();
    let proxy: FernWmV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("fern_wm_v1 is not offered (not started by fern-comp?): {e}"))?;
    let mut app = App {
        wm: Wm::new(),
        proxy,
        queue: Vec::new(),
    };
    loop {
        queue.blocking_dispatch(&mut app)?;
        if !app.queue.is_empty() {
            for cmd in std::mem::take(&mut app.queue) {
                send(&app.proxy, cmd);
            }
            app.proxy.commit();
            conn.flush()?;
        }
    }
}

fn send(p: &FernWmV1, cmd: Cmd) {
    use fern_wm_v1::{Align, Decoration, Edges, Modifiers, Motion, State};
    match cmd {
        Cmd::Configure { id, w, h, states } => {
            p.configure(id, w, h, State::from_bits_truncate(states));
        }
        Cmd::Place {
            id,
            x,
            y,
            scale,
            alpha,
            motion,
        } => {
            let m = match motion {
                1 => Motion::Snappy,
                2 => Motion::Smooth,
                3 => Motion::Gentle,
                _ => Motion::Instant,
            };
            p.place(id, x, y, scale, alpha, m);
        }
        Cmd::Decoration { id, titlebar } => {
            p.decoration(
                id,
                if titlebar {
                    Decoration::Titlebar
                } else {
                    Decoration::None
                },
            );
        }
        Cmd::Visible { id, visible } => p.visible(id, u32::from(visible)),
        Cmd::Stack(ids) => p.stack(ids.iter().flat_map(|i| i.to_ne_bytes()).collect()),
        Cmd::Focus(id) => p.focus(id),
        Cmd::Close(id) => p.close(id),
        Cmd::BeginMove(id) => p.begin_move(id),
        Cmd::BeginResize(id, edges) => p.begin_resize(id, Edges::from_bits_truncate(edges)),
        Cmd::Save(id, data) => p.save(id, data),
        Cmd::SaveWm(data) => p.save_wm(data),
        Cmd::Bind { id, mods, keysym } => p.bind(id, Modifiers::from_bits_truncate(mods), keysym),
        Cmd::GrabPointer(g) => p.grab_pointer(u32::from(g)),
        Cmd::GrabKeyboard(g) => p.grab_keyboard(u32::from(g)),
        Cmd::OverlayClear => p.overlay_clear(),
        Cmd::OverlayBeneath(id) => p.overlay_beneath(id),
        Cmd::OverlayRect {
            x,
            y,
            w,
            h,
            radius,
            rgba,
        } => p.overlay_rect(x, y, w, h, radius, rgba),
        Cmd::OverlayWallpaper { x, y, w, h, radius } => p.overlay_wallpaper(x, y, w, h, radius),
        Cmd::OverlayMirror { id, x, y, w, h } => p.overlay_mirror(id, x, y, w, h),
        Cmd::OverlayText {
            x,
            y,
            width,
            size,
            rgba,
            align,
            text,
        } => {
            let a = match align {
                1 => Align::Center,
                2 => Align::Right,
                _ => Align::Left,
            };
            p.overlay_text(x, y, width, size, rgba, a, text);
        }
    }
}

fn bits<T: Into<u32> + Copy>(e: WEnum<T>) -> u32 {
    match e {
        WEnum::Value(v) => v.into(),
        WEnum::Unknown(u) => u,
    }
}

fn flags<T: Copy>(e: WEnum<T>, f: impl Fn(T) -> u32) -> u32 {
    match e {
        WEnum::Value(v) => f(v),
        WEnum::Unknown(u) => u,
    }
}

impl Dispatch<FernWmV1, ()> for App {
    // One arm per protocol event: a table, long by nature.
    #[allow(clippy::too_many_lines)]
    fn event(
        app: &mut Self,
        _proxy: &FernWmV1,
        event: fern_wm_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use fern_wm_v1::Event as E;
        let ev = match event {
            E::Output {
                id,
                x,
                y,
                width,
                height,
            } => Event::Output {
                id,
                x,
                y,
                w: width,
                h: height,
            },
            E::OutputRemoved { id } => Event::OutputRemoved(id),
            E::WorkArea {
                id,
                x,
                y,
                width,
                height,
            } => Event::WorkArea {
                id,
                x,
                y,
                w: width,
                h: height,
            },
            E::Window {
                id,
                app_id,
                title,
                parent,
            } => Event::Window {
                id,
                app_id,
                title,
                parent,
            },
            E::WindowTitle { id, title } => Event::Title(id, title),
            E::WindowAppId { id, app_id } => Event::AppId(id, app_id),
            E::WindowHints {
                id,
                min_width,
                min_height,
                max_width,
                max_height,
            } => Event::Hints {
                id,
                min: (min_width, min_height),
                max: (max_width, max_height),
            },
            E::WindowSize { id, width, height } => Event::Size {
                id,
                w: width,
                h: height,
            },
            E::WindowClosed { id } => Event::Closed(id),
            E::WindowRequest { id, kind, edges } => Event::Request {
                id,
                kind: bits(kind),
                edges: flags(edges, |e| e.bits()),
            },
            E::Restore {
                id,
                x,
                y,
                width,
                height,
                states,
                visible,
                data,
            } => Event::Restore {
                id,
                x,
                y,
                w: width,
                h: height,
                states: flags(states, |s| s.bits()),
                visible: visible != 0,
                data,
            },
            E::RestoreWm { data } => Event::RestoreWm(data),
            E::WindowIcon {
                id,
                x,
                y,
                width,
                height,
            } => Event::Icon {
                id,
                rect: (width > 0 && height > 0).then(|| policy::Rect::new(x, y, width, height)),
            },
            E::Done => Event::Done,
            E::WindowDecoration {
                id,
                server_side,
                titlebar_height,
            } => Event::Decoration {
                id,
                server_side: server_side != 0,
                titlebar: titlebar_height,
            },
            E::PointerButton {
                window,
                part,
                x,
                y,
                button,
                pressed,
                modifiers,
                clicks,
                edges,
            } => Event::Button {
                window,
                part: bits(part),
                x,
                y,
                button,
                pressed: pressed != 0,
                mods: flags(modifiers, |m| m.bits()),
                clicks,
                edges: flags(edges, |e| e.bits()),
            },
            E::PointerMotion { x, y } => Event::Motion { x, y },
            E::Key {
                keysym,
                pressed,
                modifiers,
            } => Event::Key {
                keysym,
                pressed: pressed != 0,
                mods: flags(modifiers, |m| m.bits()),
            },
            E::Binding { id } => Event::Binding(id),
            E::MoveUpdate {
                id,
                x,
                y,
                pointer_x,
                pointer_y,
            } => Event::MoveUpdate {
                id,
                x,
                y,
                px: pointer_x,
                py: pointer_y,
            },
            E::MoveEnd {
                id,
                x,
                y,
                pointer_x,
                pointer_y,
            } => Event::MoveEnd {
                id,
                x,
                y,
                px: pointer_x,
                py: pointer_y,
            },
            E::ResizeEnd {
                id,
                x,
                y,
                width,
                height,
            } => Event::ResizeEnd {
                id,
                x,
                y,
                w: width,
                h: height,
            },
            _ => return,
        };
        let cmds = app.wm.handle(ev);
        app.queue.extend(cmds);
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
