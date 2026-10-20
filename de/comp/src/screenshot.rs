//! Screenshots for the shell (`fern_shell_v1.screenshot`, trusted clients
//! only): the next frame of a display, drawn again offscreen without the
//! pointer, cut to the asked rectangle and handed over in a memfd as RGBA.
//! The shell saves and names the file.

use std::io::Write as _;
use std::os::fd::AsFd as _;

use fern_wm_proto::shell::server::fern_shell_v1::FernShellV1;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, Offscreen,
            damage::OutputDamageTracker,
            gles::{GlesRenderer, GlesTexture},
        },
    },
    reexports::rustix::fs::{MemfdFlags, memfd_create},
    utils::{Buffer, Logical, Physical, Point, Rectangle, Size, Transform},
};

use crate::{Comp, backend::BACKGROUND, render::OutputElement};

/// A screenshot asked for and not yet taken.
#[derive(Debug)]
pub struct Request {
    pub shell: FernShellV1,
    pub id: u32,
    /// Global logical; empty for the whole display under the pointer.
    pub area: Rectangle<i32, Logical>,
}

impl Comp {
    /// Queues a screenshot for the next frame.
    pub fn screenshot_request(&mut self, req: Request) {
        self.screenshots.push(req);
        self.request_redraw();
    }

    /// Takes the queued requests this display answers: those whose
    /// rectangle's centre is on it, or (empty rectangles) the one under the
    /// pointer.
    pub fn take_screenshots(&mut self, display: Rectangle<i32, Logical>) -> Vec<Request> {
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location().to_i32_round());
        let mine = |r: &Request| {
            if r.area.is_empty() {
                pointer.is_none_or(|p| display.contains(p))
            } else {
                display.contains(r.area.loc + Point::from((r.area.size.w / 2, r.area.size.h / 2)))
            }
        };
        let (take, keep) = std::mem::take(&mut self.screenshots)
            .into_iter()
            .partition(mine);
        self.screenshots = keep;
        take
    }
}

/// Draws `elements` (a display of `display` size at scale 1, no pointer)
/// offscreen and answers each request with its part.
pub fn answer(
    renderer: &mut GlesRenderer,
    elements: &[OutputElement],
    display: Rectangle<i32, Logical>,
    requests: Vec<Request>,
) {
    if requests.is_empty() {
        return;
    }
    let size: Size<i32, Physical> = (display.size.w, display.size.h).into();
    let mut pixels = |region: Rectangle<i32, Buffer>| -> Result<Vec<u8>, String> {
        let mut texture: GlesTexture = renderer
            .create_buffer(Fourcc::Abgr8888, (size.w, size.h).into())
            .map_err(|e| e.to_string())?;
        let mut tracker = OutputDamageTracker::new(size, 1.0, Transform::Normal);
        {
            let mut target = renderer.bind(&mut texture).map_err(|e| e.to_string())?;
            tracker
                .render_output(renderer, &mut target, 0, elements, BACKGROUND)
                .map_err(|e| format!("{e:?}"))?;
        }
        let target = renderer.bind(&mut texture).map_err(|e| e.to_string())?;
        let mapping = renderer
            .copy_framebuffer(&target, region, Fourcc::Abgr8888)
            .map_err(|e| e.to_string())?;
        drop(target);
        Ok(renderer
            .map_texture(&mapping)
            .map_err(|e| e.to_string())?
            .to_vec())
    };
    for req in requests {
        let local = if req.area.is_empty() {
            Rectangle::from_size(display.size)
        } else {
            let Some(r) = req.area.intersection(display) else {
                req.shell.screenshot_failed(req.id);
                continue;
            };
            Rectangle::new(r.loc - display.loc, r.size)
        };
        // Scale 1: logical, physical and buffer pixels are the same.
        let region: Rectangle<i32, Buffer> = Rectangle::new(
            (local.loc.x, local.loc.y).into(),
            (local.size.w, local.size.h).into(),
        );
        let sent = pixels(region).and_then(|rgba| send(&req, &rgba, region.size));
        if let Err(e) = sent {
            tracing::warn!(id = req.id, "screenshot: {e}");
            req.shell.screenshot_failed(req.id);
        }
    }
}

fn send(req: &Request, rgba: &[u8], size: Size<i32, Buffer>) -> Result<(), String> {
    let (w, h) = (
        u32::try_from(size.w).map_err(|e| e.to_string())?,
        u32::try_from(size.h).map_err(|e| e.to_string())?,
    );
    if rgba.len() != w as usize * h as usize * 4 {
        return Err(format!("{} bytes for {w}x{h}", rgba.len()));
    }
    let fd = memfd_create("fern-screenshot", MemfdFlags::CLOEXEC).map_err(|e| e.to_string())?;
    let mut file = std::fs::File::from(fd);
    file.write_all(rgba).map_err(|e| e.to_string())?;
    req.shell.screenshot_done(req.id, file.as_fd(), w, h);
    Ok(())
}
