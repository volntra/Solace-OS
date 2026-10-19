//! Background blur ("materials"): what lies behind a window that asked
//! for blur is rendered offscreen, blurred with the dual-Kawase filter
//! (Marius Bjørge, ARM, SIGGRAPH 2015: a few half-resolution down and
//! up passes, far cheaper than a Gaussian of the same reach), slightly
//! saturated and dithered, and drawn under the window clipped to its
//! rounded rectangle.
//!
//! Only the area under the blur (plus the filter's reach) is rendered,
//! and the result is kept until something behind it changes, so a still
//! dock costs nothing per frame.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Color32F, Frame, Offscreen, Renderer, Texture,
            element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
            gles::{
                GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform,
                UniformName, UniformType,
            },
            utils::{CommitCounter, OpaqueRegions, draw_render_elements},
        },
    },
    desktop::Window,
    utils::{Buffer, Physical, Point, Rectangle, Scale, Size, Transform},
};

/// How strong a material's blur is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurStyle {
    /// Down/up pass pairs; each one doubles the reach.
    pub passes: u32,
    /// Sample spread per pass, in pixels of that pass's texture.
    pub offset: f32,
    /// Colour boost of the blurred backdrop (1.0: none), as glass does.
    pub saturation: f32,
    /// Dither amplitude, against banding in smooth gradients.
    pub noise: f32,
}

/// The standard material blur (HIG: Materials).
pub const MATERIAL: BlurStyle = BlurStyle {
    passes: 3,
    offset: 4.0,
    saturation: 1.35,
    noise: 0.012,
};

impl BlurStyle {
    /// How far (output pixels) content outside a blurred area still
    /// shows inside it: the area rendered around it.
    pub fn reach(&self) -> i32 {
        // Each pass samples up to 2 * offset texels of a texture
        // 2^pass times smaller than the output, down and up again.
        #[allow(clippy::cast_possible_truncation)]
        let reach = (f64::from(self.offset) * f64::from(1u32 << (self.passes + 1))).ceil() as i32;
        reach
    }
}

const DOWN_FRAG: &str = r"
//_DEFINES_
#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif
uniform float alpha;
varying vec2 v_coords;
// Half a texel of the source texture, and the sample spread.
uniform vec2 halfpixel;
uniform float offset;

void main() {
    vec2 o = halfpixel * offset;
    vec4 sum = texture2D(tex, v_coords) * 4.0;
    sum += texture2D(tex, v_coords - o);
    sum += texture2D(tex, v_coords + o);
    sum += texture2D(tex, v_coords + vec2(o.x, -o.y));
    sum += texture2D(tex, v_coords - vec2(o.x, -o.y));
    gl_FragColor = sum / 8.0 * alpha;
}
";

const UP_FRAG: &str = r"
//_DEFINES_
#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif
uniform float alpha;
varying vec2 v_coords;
uniform vec2 halfpixel;
uniform float offset;
// Applied on the last pass only (1.0 and 0.0 otherwise).
uniform float saturation;
uniform float noise;

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

void main() {
    vec2 o = halfpixel * offset;
    vec4 sum = texture2D(tex, v_coords + vec2(-o.x * 2.0, 0.0));
    sum += texture2D(tex, v_coords + vec2(-o.x, o.y)) * 2.0;
    sum += texture2D(tex, v_coords + vec2(0.0, o.y * 2.0));
    sum += texture2D(tex, v_coords + vec2(o.x, o.y)) * 2.0;
    sum += texture2D(tex, v_coords + vec2(o.x * 2.0, 0.0));
    sum += texture2D(tex, v_coords + vec2(o.x, -o.y)) * 2.0;
    sum += texture2D(tex, v_coords + vec2(0.0, -o.y * 2.0));
    sum += texture2D(tex, v_coords + vec2(-o.x, -o.y)) * 2.0;
    vec4 color = sum / 12.0;
    float luma = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    color.rgb = clamp(mix(vec3(luma), color.rgb, saturation), 0.0, color.a);
    color.rgb += (hash(gl_FragCoord.xy) - 0.5) * noise * color.a;
    gl_FragColor = color * alpha;
}
";

/// The blur passes' shaders.
#[derive(Debug, Clone)]
pub struct BlurShaders {
    down: GlesTexProgram,
    up: GlesTexProgram,
}

impl BlurShaders {
    /// # Errors
    /// The GLSL does not compile on this driver.
    pub fn compile(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let common = [
            UniformName::new("halfpixel", UniformType::_2f),
            UniformName::new("offset", UniformType::_1f),
        ];
        let down = renderer.compile_custom_texture_shader(DOWN_FRAG, &common)?;
        let mut up_uniforms = common.to_vec();
        up_uniforms.push(UniformName::new("saturation", UniformType::_1f));
        up_uniforms.push(UniformName::new("noise", UniformType::_1f));
        let up = renderer.compile_custom_texture_shader(UP_FRAG, &up_uniforms)?;
        Ok(BlurShaders { down, up })
    }
}

/// Sizes of the down-sampled levels of an area of `size`: halved per
/// pass, never below one pixel.
pub fn level_sizes(size: Size<i32, Physical>, passes: u32) -> Vec<Size<i32, Physical>> {
    (1..=passes)
        .map(|i| {
            let d = 1i32 << i;
            Size::from(((size.w + d - 1) / d, (size.h + d - 1) / d))
        })
        .collect()
}

/// The area to render for blurring `rects`: their bounds plus the
/// filter's reach, within the output. `None` when nothing is visible.
pub fn capture_area(
    rects: &[Rectangle<i32, Physical>],
    reach: i32,
    output: Size<i32, Physical>,
) -> Option<Rectangle<i32, Physical>> {
    let bounds = rects.iter().copied().reduce(Rectangle::merge)?;
    let grown = Rectangle::new(
        bounds.loc - Point::from((reach, reach)),
        bounds.size + Size::from((2 * reach, 2 * reach)),
    );
    grown
        .intersection(Rectangle::from_size(output))
        .filter(|r| !r.is_empty())
}

/// What a blurred result was made from: every element behind it that
/// touches the captured area, with its version and place.
type Key = Vec<(Id, CommitCounter, Rectangle<i32, Physical>)>;

/// How many times a backdrop has been blurred, process-wide: lets tests
/// check that unchanged backdrops are reused.
static RENDERS: AtomicU64 = AtomicU64::new(0);

/// Backdrops blurred so far (see [`RENDERS`]).
pub fn renders() -> u64 {
    RENDERS.load(Ordering::Relaxed)
}

/// A window's blur textures and the key they were made from.
#[derive(Default)]
struct Cache {
    capture: Option<GlesTexture>,
    levels: Vec<GlesTexture>,
    result: Option<GlesTexture>,
    area: Option<Rectangle<i32, Physical>>,
    key: Key,
    commit: CommitCounter,
    ids: Vec<Id>,
}

#[derive(Default)]
struct BlurCache(RefCell<Cache>);

/// A texture of `size`, reusing `slot` when it already has that size.
fn texture(
    renderer: &mut GlesRenderer,
    slot: &mut Option<GlesTexture>,
    size: Size<i32, Physical>,
) -> Result<GlesTexture, GlesError> {
    let size = Size::<i32, Buffer>::from((size.w, size.h));
    if let Some(t) = slot
        && t.size() == size
    {
        return Ok(t.clone());
    }
    let t: GlesTexture = renderer.create_buffer(Fourcc::Abgr8888, size)?;
    *slot = Some(t.clone());
    Ok(t)
}

/// Draws `src_rect` of `src` over the whole of `dst` with `program`.
fn pass(
    renderer: &mut GlesRenderer,
    src: &GlesTexture,
    src_rect: Rectangle<f64, Buffer>,
    dst: &mut GlesTexture,
    program: &GlesTexProgram,
    uniforms: &[Uniform<'_>],
) -> Result<(), GlesError> {
    let size = dst.size();
    let full = Rectangle::<i32, Physical>::from_size((size.w, size.h).into());
    let mut target = renderer.bind(dst)?;
    let mut frame = renderer.render(&mut target, full.size, Transform::Normal)?;
    frame.clear(Color32F::TRANSPARENT, &[full])?;
    frame.render_texture_from_to(
        src,
        src_rect,
        full,
        &[full],
        &[],
        Transform::Normal,
        1.0,
        Some(program),
        uniforms,
    )?;
    // The next pass samples this texture in the same GL context, which
    // orders the commands; no fence to wait for.
    let _ = frame.finish()?;
    Ok(())
}

fn halfpixel(t: &GlesTexture) -> [f32; 2] {
    let s = t.size();
    #[allow(clippy::cast_precision_loss)]
    let hp = [0.5 / s.w.max(1) as f32, 0.5 / s.h.max(1) as f32];
    hp
}

/// Renders `behind` (front to back) inside `area` and blurs it; returns
/// the blurred texture, `area`-sized.
#[allow(clippy::too_many_arguments)]
fn blur<E: RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    shaders: &BlurShaders,
    style: BlurStyle,
    cache: &mut Cache,
    behind: &[E],
    area: Rectangle<i32, Physical>,
    output: Size<i32, Physical>,
    scale: Scale<f64>,
) -> Result<GlesTexture, GlesError> {
    let mut capture = texture(renderer, &mut cache.capture, output)?;
    {
        let mut target = renderer.bind(&mut capture)?;
        let mut frame = renderer.render(&mut target, output, Transform::Normal)?;
        frame.clear(Color32F::new(0.0, 0.0, 0.0, 1.0), &[area])?;
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, scale, behind, &[area])?;
        let _ = frame.finish()?;
    }
    let sizes = level_sizes(area.size, style.passes);
    if cache.levels.len() != sizes.len()
        || cache
            .levels
            .iter()
            .zip(&sizes)
            .any(|(t, s)| t.size() != (s.w, s.h).into())
    {
        cache.levels.clear();
        for s in &sizes {
            cache.levels.push(texture(renderer, &mut None, *s)?);
        }
    }
    let offset = Uniform::new("offset", style.offset);
    // Down: the captured area, then each level from the one above.
    let mut src = capture;
    let a = area.to_f64();
    let mut src_rect =
        Rectangle::<f64, Buffer>::new((a.loc.x, a.loc.y).into(), (a.size.w, a.size.h).into());
    for level in &mut cache.levels {
        let hp = Uniform::new("halfpixel", halfpixel(&src));
        pass(
            renderer,
            &src,
            src_rect,
            level,
            &shaders.down,
            &[hp, offset.clone()],
        )?;
        src = level.clone();
        src_rect = Rectangle::from_size(src.size().to_f64());
    }
    // Up: back through the levels, the last pass into the result.
    let mut result = texture(renderer, &mut cache.result, area.size)?;
    let n = cache.levels.len();
    for i in (0..n).rev() {
        let last = i == 0;
        let mut dst = if last {
            result.clone()
        } else {
            cache.levels[i - 1].clone()
        };
        let uniforms = [
            Uniform::new("halfpixel", halfpixel(&src)),
            offset.clone(),
            Uniform::new("saturation", if last { style.saturation } else { 1.0 }),
            Uniform::new("noise", if last { style.noise } else { 0.0 }),
        ];
        pass(renderer, &src, src_rect, &mut dst, &shaders.up, &uniforms)?;
        src = dst;
        src_rect = Rectangle::from_size(src.size().to_f64());
    }
    if n == 0 {
        // No passes: a plain copy (the down filter with no spread).
        let uniforms = [Uniform::new("halfpixel", [0.0f32, 0.0]), offset];
        pass(
            renderer,
            &src,
            src_rect,
            &mut result,
            &shaders.down,
            &uniforms,
        )?;
    }
    Ok(result)
}

/// A blurred backdrop under part of a window, clipped to the window's
/// rounded rectangle.
#[derive(Debug, Clone)]
pub struct Backdrop {
    id: Id,
    commit: CommitCounter,
    texture: GlesTexture,
    src: Rectangle<f64, Buffer>,
    geometry: Rectangle<i32, Physical>,
    program: GlesTexProgram,
    /// The window's rounded rectangle relative to `geometry`.
    clip: Rectangle<f64, Physical>,
    radius: f32,
    fb_height: f32,
}

impl Element for Backdrop {
    fn id(&self) -> &Id {
        &self.id
    }
    fn current_commit(&self) -> CommitCounter {
        self.commit
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.src
    }
    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }
    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }
    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl RenderElement<GlesRenderer> for Backdrop {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let uniforms = crate::render::clip_uniforms(
            frame.transformation(),
            dst.loc,
            self.clip,
            self.radius,
            self.fb_height,
        );
        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            &[],
            Transform::Normal,
            1.0,
            Some(&self.program),
            &uniforms,
        )
    }

    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        None
    }
}

/// Where a window's blur goes and how it is clipped.
#[derive(Debug, Clone, Copy)]
pub struct Clip {
    /// The window's rounded rectangle, output pixels.
    pub window: Rectangle<f64, Physical>,
    pub radius: f32,
    pub fb_height: f32,
}

/// The backdrop elements of `window` for its blurred `rects` (output
/// pixels), made from `behind` (front to back). Reuses the last result
/// when nothing behind changed.
///
/// # Errors
/// Offscreen rendering failed (e.g. out of GPU memory).
#[allow(clippy::too_many_arguments)]
pub fn backdrops<E: RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    shaders: &BlurShaders,
    program: &GlesTexProgram,
    window: &Window,
    rects: &[Rectangle<i32, Physical>],
    clip: Clip,
    behind: &[E],
    output: Size<i32, Physical>,
    scale: Scale<f64>,
) -> Result<Vec<Backdrop>, GlesError> {
    let style = MATERIAL;
    let Some(area) = capture_area(rects, style.reach(), output) else {
        return Ok(Vec::new());
    };
    let key: Key = behind
        .iter()
        .map(|e| (e.id().clone(), e.current_commit(), e.geometry(scale)))
        .filter(|(_, _, g)| g.overlaps(area))
        .collect();
    let cache = window.user_data().get_or_insert(BlurCache::default);
    let mut cache = cache.0.borrow_mut();
    let unchanged = cache.area == Some(area) && cache.key == key;
    let texture = if let (Some(t), true) = (&cache.result, unchanged) {
        t.clone()
    } else {
        let t = blur(
            renderer, shaders, style, &mut cache, behind, area, output, scale,
        )?;
        tracing::debug!(?area, "blurred backdrop re-rendered");
        RENDERS.fetch_add(1, Ordering::Relaxed);
        cache.area = Some(area);
        cache.key = key;
        cache.commit.increment();
        t
    };
    while cache.ids.len() < rects.len() {
        cache.ids.push(Id::new());
    }
    Ok(rects
        .iter()
        .zip(&cache.ids)
        .filter_map(|(rect, id)| {
            let rect = rect.intersection(area)?;
            let local = rect.loc - area.loc;
            Some(Backdrop {
                id: id.clone(),
                commit: cache.commit,
                texture: texture.clone(),
                src: Rectangle::new(
                    (f64::from(local.x), f64::from(local.y)).into(),
                    (f64::from(rect.size.w), f64::from(rect.size.h)).into(),
                ),
                geometry: rect,
                program: program.clone(),
                clip: Rectangle::new(clip.window.loc - rect.loc.to_f64(), clip.window.size),
                radius: clip.radius,
                fb_height: clip.fb_height,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_halve_and_round_up() {
        let sizes = level_sizes((301, 100).into(), 3);
        assert_eq!(
            sizes,
            vec![(151, 50).into(), (76, 25).into(), (38, 13).into()]
        );
        assert_eq!(level_sizes((1, 1).into(), 4), vec![(1, 1).into(); 4]);
    }

    #[test]
    fn the_capture_area_grows_by_the_reach_within_the_output() {
        let r = |x, y, w, h| Rectangle::<i32, Physical>::new((x, y).into(), (w, h).into());
        let out = Size::from((1000, 800));
        assert_eq!(
            capture_area(&[r(100, 100, 200, 50)], 64, out),
            Some(r(36, 36, 328, 178))
        );
        // Two rectangles: their bounds; clamped at the output's edge.
        assert_eq!(
            capture_area(&[r(0, 700, 50, 50), r(900, 760, 100, 40)], 64, out),
            Some(r(0, 636, 1000, 164))
        );
        assert_eq!(capture_area(&[], 64, out), None);
        assert_eq!(capture_area(&[r(2000, 0, 10, 10)], 64, out), None);
    }

    #[test]
    fn reach_doubles_per_pass() {
        let s = BlurStyle {
            passes: 2,
            offset: 3.0,
            ..MATERIAL
        };
        assert_eq!(s.reach(), 24);
        assert_eq!(BlurStyle { passes: 3, ..s }.reach(), 48);
    }
}
