//! Nested backend: sol-comp runs as a window inside another desktop
//! (X11 or Wayland), which is how it is developed. Adapted from Smithay's
//! smallvil example (v0.7.0, MIT licence).

use std::path::PathBuf;
use std::time::Duration;

use smithay::{
    backend::{
        renderer::{damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::Transform,
};

use crate::{
    Comp, LoopData,
    render::{self, Shaders, Wallpaper},
};

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
    let shaders = Shaders::compile(backend.renderer())?;
    let wallpaper = load_wallpaper();

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
                    let logical = size.to_logical(1);
                    let age = backend.buffer_age().unwrap_or(0);
                    let mut damage = None;
                    {
                        let Ok((renderer, mut framebuffer)) = backend.bind() else {
                            return;
                        };
                        let elements = render::output_elements(
                            renderer,
                            &shaders,
                            &state.space,
                            wallpaper.as_ref(),
                            logical,
                            1.0,
                        );
                        match damage_tracker.render_output(
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
                    // Present only what changed; with no damage the frame
                    // is skipped entirely.
                    if let Some(damage) = damage
                        && let Err(e) = backend.submit(Some(&damage))
                    {
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

/// The wallpaper: `SOL_WALLPAPER`, the installed default, or the
/// development build's render (`just wallpapers`). Missing is not fatal.
fn load_wallpaper() -> Option<Wallpaper> {
    let candidates: Vec<PathBuf> = std::env::var_os("SOL_WALLPAPER")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            PathBuf::from("/usr/share/sol/wallpapers/dusk.png"),
            PathBuf::from("build/wallpapers/dusk.png"),
        ])
        .collect();
    for path in &candidates {
        if !path.exists() {
            continue;
        }
        match render::load_wallpaper(path) {
            Ok(w) => return Some(w),
            Err(e) => tracing::warn!("wallpaper: {e}"),
        }
    }
    tracing::info!("no wallpaper found; using the plain background");
    None
}
