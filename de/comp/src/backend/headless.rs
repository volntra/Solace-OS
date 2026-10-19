//! Headless backend: renders into an offscreen texture with no display
//! and no GPU required (Mesa's surfaceless EGL platform; llvmpipe when
//! there is no GPU). Used by the pixel tests, and later by screenshots
//! in automated smoke tests.

use std::path::Path;

use smithay::{
    backend::{
        allocator::Fourcc,
        egl::{EGLContext, EGLDisplay, native::EGLSurfacelessDisplay},
        renderer::{
            Bind, ExportMem, Offscreen, Texture,
            damage::OutputDamageTracker,
            gles::{GlesRenderer, GlesTexture},
        },
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    utils::{Physical, Rectangle, Size, Transform},
};

use crate::{
    Comp,
    backend::BACKGROUND,
    render::{self, Shaders, Wallpaper},
};

/// An output that exists only in memory.
#[derive(Debug)]
pub struct Headless {
    renderer: GlesRenderer,
    output: Output,
    damage_tracker: OutputDamageTracker,
    shaders: Shaders,
    wallpaper: Option<Wallpaper>,
    target: GlesTexture,
    size: Size<i32, Physical>,
    /// Frames rendered into `target` so far (0: its contents are undefined).
    frames: u64,
}

/// A screenshot: RGBA, 8 bits per channel, rows from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// The pixel at (x, y); transparent black outside the image.
    pub fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
            return [0; 4];
        };
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    /// Saves the image as PNG.
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        writer
            .write_image_data(&self.rgba)
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}

impl Headless {
    /// Creates a `width` x `height` output and maps it into the space.
    ///
    /// # Errors
    /// No EGL implementation with the surfaceless platform, or the
    /// shaders do not compile.
    pub fn new(
        state: &mut Comp,
        (width, height): (i32, i32),
        wallpaper: Option<Wallpaper>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // SAFETY: `EGLSurfacelessDisplay` names EGL's default display on
        // the surfaceless platform; there is no native handle that could
        // dangle.
        #[allow(unsafe_code)]
        let display = unsafe { EGLDisplay::new(EGLSurfacelessDisplay)? };
        let context = EGLContext::new(&display)?;
        // SAFETY: the context was just created and is used by this
        // renderer only.
        #[allow(unsafe_code)]
        let mut renderer = unsafe { GlesRenderer::new(context)? };
        let size = Size::from((width, height));
        let target: GlesTexture =
            renderer.create_buffer(Fourcc::Abgr8888, Size::from((width, height)))?;
        let shaders = Shaders::compile(&mut renderer)?;

        let output = Output::new(
            "headless".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Solace".into(),
                model: "Headless".into(),
            },
        );
        let _global = output.create_global::<Comp>(&state.display_handle);
        let mode = Mode {
            size,
            refresh: 60_000,
        };
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            None,
            Some((0, 0).into()),
        );
        output.set_preferred(mode);
        state.space.map_output(&output, (0, 0));
        Ok(Headless {
            renderer,
            damage_tracker: OutputDamageTracker::from_output(&output),
            output,
            shaders,
            wallpaper,
            target,
            size,
            frames: 0,
        })
    }

    pub fn output(&self) -> &Output {
        &self.output
    }

    /// Renders a frame and tells clients it was shown. Returns whether
    /// anything on screen changed.
    ///
    /// # Errors
    /// Rendering failed.
    pub fn render(&mut self, state: &mut Comp) -> Result<bool, Box<dyn std::error::Error>> {
        let logical = self.size.to_logical(1);
        let elements = render::output_elements(
            &mut self.renderer,
            &self.shaders,
            &state.space,
            self.wallpaper.as_ref(),
            Rectangle::from_size(logical),
            1.0,
        );
        // The texture keeps the previous frame, so after the first frame
        // only the damage needs redrawing.
        let age = usize::from(self.frames > 0);
        let mut target = self.renderer.bind(&mut self.target)?;
        let result = self.damage_tracker.render_output(
            &mut self.renderer,
            &mut target,
            age,
            &elements,
            BACKGROUND,
        )?;
        let changed = result.damage.is_some_and(|d| !d.is_empty());
        self.frames += 1;
        state.frame_shown(&self.output);
        Ok(changed)
    }

    /// Reads back the last rendered frame.
    ///
    /// # Errors
    /// Reading pixels from the GPU failed.
    pub fn screenshot(&mut self) -> Result<Image, Box<dyn std::error::Error>> {
        let size = self.target.size();
        let target = self.renderer.bind(&mut self.target)?;
        let mapping = self.renderer.copy_framebuffer(
            &target,
            Rectangle::from_size(size),
            Fourcc::Abgr8888,
        )?;
        drop(target);
        let bytes = self.renderer.map_texture(&mapping)?;
        Ok(Image {
            width: u32::try_from(size.w)?,
            height: u32::try_from(size.h)?,
            rgba: bytes.to_vec(),
        })
    }
}
