//! Nested backend: fern-comp runs as a window inside another desktop
//! (X11 or Wayland), which is how it is developed. Adapted from Smithay's
//! smallvil example (v0.7.0, MIT licence).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use smithay::{
    backend::{
        renderer::{ImportDma, damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::{
        EventLoop, LoopHandle,
        timer::{TimeoutAction, Timer},
    },
    utils::{Rectangle, Transform},
};

#[cfg(feature = "drm")]
use smithay::wayland::dmabuf::DmabufFeedbackBuilder;

use crate::{
    Comp, LoopData,
    backend::BACKGROUND,
    render::{self, Shaders},
};

/// When nothing changed, the next frame is checked after this long (the
/// nested output's 60 Hz) instead of immediately.
const IDLE_FRAME: Duration = Duration::from_micros(16_667);

type Backend = Rc<RefCell<WinitGraphicsBackend<GlesRenderer>>>;

/// Everything the nested output needs between frames.
struct Nested<'l> {
    backend: Backend,
    output: Output,
    damage_tracker: OutputDamageTracker,
    shaders: Shaders,
    wallpapers: render::Wallpapers,
    handle: LoopHandle<'l, LoopData>,
}

/// Opens the nested window and drives rendering from its redraw events.
///
/// # Errors
/// No display to nest in, or no usable EGL/OpenGL ES.
pub fn init(
    event_loop: &mut EventLoop<'_, LoopData>,
    data: &mut LoopData,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut backend, winit) = winit::init::<GlesRenderer>()?;
    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };
    let output = Output::new(
        "nested".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Fern".into(),
            model: "Nested".into(),
        },
    );
    let _global = output.create_global::<Comp>(&data.display_handle);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    data.state.space.map_output(&output, (0, 0));
    let shaders = Shaders::compile(backend.renderer())?;
    gpu_clients(&mut backend, &mut data.state);
    let backend_rc = Rc::new(RefCell::new(backend));
    let weak = Rc::downgrade(&backend_rc);
    data.state.dmabuf_import = Some(Box::new(move |dmabuf| {
        weak.upgrade().is_some_and(|b| {
            b.try_borrow_mut()
                .is_ok_and(|mut b| b.renderer().import_dmabuf(dmabuf, None).is_ok())
        })
    }));
    let mut nested = Nested {
        backend: Rc::clone(&backend_rc),
        damage_tracker: OutputDamageTracker::from_output(&output),
        output,
        shaders,
        wallpapers: render::Wallpapers::new(render::default_wallpaper()),
        handle: event_loop.handle(),
    };

    event_loop
        .handle()
        .insert_source(winit, move |event, (), data| match event {
            WinitEvent::Resized { size, .. } => {
                nested.output.change_current_state(
                    Some(Mode {
                        size,
                        refresh: 60_000,
                    }),
                    None,
                    None,
                    None,
                );
                data.state.output_changed(&nested.output);
            }
            WinitEvent::Input(event) => data.state.process_input_event(event),
            WinitEvent::Redraw => nested.redraw(data),
            WinitEvent::CloseRequested => data.state.loop_signal.stop(),
            WinitEvent::Focus(_) => (),
        })?;
    Ok(())
}

/// GPU buffers from clients: linux-dmabuf with the renderer's formats,
/// with feedback when the host GPU's render node is known.
fn gpu_clients(backend: &mut WinitGraphicsBackend<GlesRenderer>, state: &mut Comp) {
    let formats = backend.renderer().dmabuf_formats();
    // Finding the render node needs the DRM support; without it, clients
    // get the formats without device feedback.
    #[cfg(feature = "drm")]
    let feedback = smithay::backend::egl::EGLDevice::device_for_display(
        backend.renderer().egl_context().display(),
    )
    .ok()
    .and_then(|d| d.try_get_render_node().ok().flatten())
    .and_then(|n| {
        DmabufFeedbackBuilder::new(n.dev_id(), formats.clone())
            .build()
            .ok()
    });
    #[cfg(not(feature = "drm"))]
    let feedback: Option<smithay::wayland::dmabuf::DmabufFeedback> = None;
    let dh = state.display_handle.clone();
    let global = match feedback {
        Some(feedback) => state
            .dmabuf_state
            .create_global_with_default_feedback::<Comp>(&dh, &feedback),
        None => state.dmabuf_state.create_global::<Comp>(&dh, formats),
    };
    state.dmabuf_global = Some(global);
}

impl Nested<'_> {
    fn redraw(&mut self, data: &mut LoopData) {
        let state = &mut data.state;
        let Ok(mut backend) = self.backend.try_borrow_mut() else {
            return;
        };
        let logical = backend.window_size().to_logical(1);
        let age = backend.buffer_age().unwrap_or(0);
        if let Some(req) = state.wallpaper_request.take() {
            self.wallpapers.change(req.as_deref());
        }
        let animating = state.advance_animations() | self.wallpapers.tick();
        let mut damage = None;
        {
            let Ok((renderer, mut framebuffer)) = backend.bind() else {
                return;
            };
            let elements = render::output_elements(
                renderer,
                &self.shaders,
                &state.space,
                Some(&self.output),
                &self.wallpapers,
                Rectangle::from_size(logical),
                1.0,
                &|w| state.visual(w),
                &state.overlay(),
            );
            match self.damage_tracker.render_output(
                renderer,
                &mut framebuffer,
                age,
                &elements,
                BACKGROUND,
            ) {
                Ok(result) => damage = result.damage.cloned(),
                Err(e) => tracing::warn!("render: {e:?}"),
            }
        }
        let shots = state.take_screenshots(Rectangle::from_size(logical));
        if !shots.is_empty() {
            let renderer = backend.renderer();
            let elements = render::output_elements(
                renderer,
                &self.shaders,
                &state.space,
                Some(&self.output),
                &self.wallpapers,
                Rectangle::from_size(logical),
                1.0,
                &|w| state.visual(w),
                &state.overlay(),
            );
            crate::screenshot::answer(renderer, &elements, Rectangle::from_size(logical), shots);
        }
        // Present only what changed. With no damage the frame is skipped
        // and the next one checked a frame later.
        let idle = damage.is_none() && !animating;
        if let Some(damage) = damage
            && let Err(e) = backend.submit(Some(&damage))
        {
            tracing::warn!("present: {e:?}");
        }
        state.frame_shown(&self.output);
        let _ = data.display_handle.flush_clients();
        if !idle {
            backend.window().request_redraw();
            return;
        }
        let later = Rc::clone(&self.backend);
        let timer = self
            .handle
            .insert_source(Timer::from_duration(IDLE_FRAME), move |_, (), _| {
                if let Ok(backend) = later.try_borrow() {
                    backend.window().request_redraw();
                }
                TimeoutAction::Drop
            });
        if let Err(e) = timer {
            tracing::warn!("frame timer: {}", e.error);
            backend.window().request_redraw();
        }
    }
}
