//! Nested backend: sol-comp runs as a window inside another desktop
//! (X11 or Wayland), which is how it is developed. Adapted from Smithay's
//! smallvil example (v0.7.0, MIT licence).

use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
        },
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Transform},
};

use crate::{Comp, LoopData};

/// The desktop background until the wallpaper is drawn: the dusk palette's
/// deepest tone.
pub const BACKGROUND: [f32; 4] = [0.075, 0.07, 0.14, 1.0];

/// Opens the nested window and drives rendering from its redraw events.
///
/// # Errors
/// No display to nest in, or no usable EGL/OpenGL ES.
pub fn init(
    event_loop: &mut EventLoop<LoopData>,
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
            make: "Solace".into(),
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
    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    event_loop
        .handle()
        .insert_source(winit, move |event, (), data| {
            let state = &mut data.state;
            match event {
                WinitEvent::Resized { size, .. } => {
                    output.change_current_state(
                        Some(Mode {
                            size,
                            refresh: 60_000,
                        }),
                        None,
                        None,
                        None,
                    );
                }
                WinitEvent::Input(event) => state.process_input_event(event),
                WinitEvent::Redraw => {
                    let size = backend.window_size();
                    let damage = Rectangle::from_size(size);
                    {
                        let Ok((renderer, mut framebuffer)) = backend.bind() else {
                            return;
                        };
                        let frame = smithay::desktop::space::render_output::<
                            _,
                            WaylandSurfaceRenderElement<GlesRenderer>,
                            _,
                            _,
                        >(
                            &output,
                            renderer,
                            &mut framebuffer,
                            1.0,
                            0,
                            [&state.space],
                            &[],
                            &mut damage_tracker,
                            BACKGROUND,
                        );
                        if let Err(e) = frame {
                            tracing::warn!("render: {e:?}");
                        }
                    }
                    if let Err(e) = backend.submit(Some(&[damage])) {
                        tracing::warn!("present: {e:?}");
                    }
                    state.space.elements().for_each(|window| {
                        window.send_frame(
                            &output,
                            state.start_time.elapsed(),
                            Some(Duration::ZERO),
                            |_, _| Some(output.clone()),
                        );
                    });
                    state.space.refresh();
                    state.popups.cleanup();
                    let _ = data.display_handle.flush_clients();
                    backend.window().request_redraw();
                }
                WinitEvent::CloseRequested => state.loop_signal.stop(),
                WinitEvent::Focus(_) => (),
            }
        })?;
    Ok(())
}
