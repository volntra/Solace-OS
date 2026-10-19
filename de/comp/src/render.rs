//! Our look on top of Smithay's GLES renderer: rounded window corners,
//! soft shadows tied to focus, and the wallpaper. Everything is drawn
//! through render elements, so the output damage tracker redraws only
//! what changed.

use std::cell::RefCell;
use std::path::Path;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Frame,
            element::{
                AsRenderElements, Element, Id, Kind, RenderElement, UnderlyingStorage,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::WaylandSurfaceRenderElement,
            },
            gles::{
                GlesError, GlesFrame, GlesPixelProgram, GlesRenderer, GlesTexProgram, Uniform,
                UniformName, UniformType, element::PixelShaderElement,
            },
            utils::{CommitCounter, DamageSet, OpaqueRegions},
        },
    },
    desktop::{Space, Window},
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

/// Window corner radius, logical pixels (HIG: Radii, window).
pub const WINDOW_RADIUS: f64 = 12.0;

/// A window's shadow: blur (sigma), downward offset, opacity; logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowStyle {
    pub sigma: f64,
    pub offset_y: f64,
    pub opacity: f32,
}

/// The focused window floats higher: a larger, darker shadow.
pub const SHADOW_FOCUSED: ShadowStyle = ShadowStyle {
    sigma: 22.0,
    offset_y: 14.0,
    opacity: 0.42,
};
pub const SHADOW_UNFOCUSED: ShadowStyle = ShadowStyle {
    sigma: 12.0,
    offset_y: 6.0,
    opacity: 0.22,
};

/// Clips textures to a rounded rectangle in output pixels. Based on
/// Smithay's built-in texture shader (same `//_DEFINES_` variants).
const ROUNDED_FRAG: &str = r"
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
// The rounded rectangle (x, y, w, h) in output pixels and its corner
// radius. gl_FragCoord is in framebuffer pixels; output y is
// fb_y.x + fb_y.y * gl_FragCoord.y (the framebuffer may be upside down).
uniform vec4 geo;
uniform float radius;
uniform vec2 fb_y;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

float rounded_rect_sdf(vec2 p, vec2 center, vec2 half_size, float r) {
    vec2 q = abs(p - center) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec4 color = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
#endif
    vec2 p = vec2(gl_FragCoord.x, fb_y.x + fb_y.y * gl_FragCoord.y);
    float d = rounded_rect_sdf(p, geo.xy + geo.zw * 0.5, geo.zw * 0.5, radius);
    // One pixel of antialiasing along the edge.
    float coverage = clamp(0.5 - d, 0.0, 1.0);
    color = color * alpha * coverage;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    gl_FragColor = color;
}
";

/// The shadow of a rounded rectangle: a Gaussian blur computed
/// analytically (exact along x with erf, four samples along y), after Evan
/// Wallace's method. Not drawn under the window itself.
const SHADOW_FRAG: &str = r"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
// The window (x, y, w, h) inside this element, the shadow's rectangle
// (offset), corner radius, blur sigma, and color (premultiplied).
uniform vec4 window;
uniform vec4 caster;
uniform float radius;
uniform float sigma;
uniform vec4 color;

vec2 erf2(vec2 x) {
    vec2 s = sign(x);
    vec2 a = abs(x);
    x = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    x *= x;
    return s - s / (x * x);
}

float gaussian(float x, float s) {
    return exp(-(x * x) / (2.0 * s * s)) / (2.5066282746 * s);
}

float shadow_x(float x, float y, float s, float corner, vec2 half_size) {
    float delta = min(half_size.y - corner - abs(y), 0.0);
    float curved = half_size.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    vec2 integral = 0.5 + 0.5 * erf2((x + vec2(-curved, curved)) * (0.70710678 / s));
    return integral.y - integral.x;
}

float shadow(vec2 lower, vec2 upper, vec2 p, float s, float corner) {
    vec2 center = (lower + upper) * 0.5;
    vec2 half_size = (upper - lower) * 0.5;
    p -= center;
    float low = p.y - half_size.y;
    float high = p.y + half_size.y;
    float start = clamp(-3.0 * s, low, high);
    float end = clamp(3.0 * s, low, high);
    float step = (end - start) / 4.0;
    float y = start + step * 0.5;
    float value = 0.0;
    for (int i = 0; i < 4; i++) {
        value += shadow_x(p.x, p.y - y, s, corner, half_size) * gaussian(y, s) * step;
        y += step;
    }
    return value;
}

float rounded_rect_sdf(vec2 p, vec2 center, vec2 half_size, float r) {
    vec2 q = abs(p - center) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 p = v_coords * size;
    float a = shadow(caster.xy, caster.xy + caster.zw, p, sigma, radius);
    float inside = clamp(0.5 - rounded_rect_sdf(p, window.xy + window.zw * 0.5, window.zw * 0.5, radius), 0.0, 1.0);
    gl_FragColor = color * (a * (1.0 - inside) * alpha);
}
";

/// The compiled shaders (one set per renderer).
#[derive(Debug, Clone)]
pub struct Shaders {
    rounded: GlesTexProgram,
    shadow: GlesPixelProgram,
}

impl Shaders {
    /// # Errors
    /// The GLSL does not compile on this driver.
    pub fn compile(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let rounded = renderer.compile_custom_texture_shader(
            ROUNDED_FRAG,
            &[
                UniformName::new("geo", UniformType::_4f),
                UniformName::new("radius", UniformType::_1f),
                UniformName::new("fb_y", UniformType::_2f),
            ],
        )?;
        let shadow = renderer.compile_custom_pixel_shader(
            SHADOW_FRAG,
            &[
                UniformName::new("window", UniformType::_4f),
                UniformName::new("caster", UniformType::_4f),
                UniformName::new("radius", UniformType::_1f),
                UniformName::new("sigma", UniformType::_1f),
                UniformName::new("color", UniformType::_4f),
            ],
        )?;
        Ok(Shaders { rounded, shadow })
    }
}

/// A window surface element clipped to the window's rounded rectangle.
#[derive(Debug)]
pub struct Rounded {
    inner: WaylandSurfaceRenderElement<GlesRenderer>,
    program: GlesTexProgram,
    /// The window's rectangle in output pixels.
    geo: Rectangle<f64, Physical>,
    radius: f32,
    fb_height: f32,
}

impl Element for Rounded {
    fn id(&self) -> &Id {
        self.inner.id()
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }
    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }
    fn transform(&self) -> Transform {
        self.inner.transform()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }
    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }
    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        // The corners are cut out, so nothing can be assumed opaque.
        OpaqueRegions::default()
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for Rounded {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        #[allow(clippy::cast_possible_truncation)]
        let geo = [
            self.geo.loc.x as f32,
            self.geo.loc.y as f32,
            self.geo.size.w as f32,
            self.geo.size.h as f32,
        ];
        frame.override_default_tex_program(
            self.program.clone(),
            vec![
                Uniform::new("geo", geo),
                Uniform::new("radius", self.radius),
                Uniform::new("fb_y", fb_y(frame.transformation(), self.fb_height)),
            ],
        );
        let drawn = self.inner.draw(frame, src, dst, damage, opaque_regions);
        frame.clear_tex_program_override();
        drawn
    }

    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        // Never scan out directly: the corners must be clipped.
        None
    }
}

/// Maps `gl_FragCoord.y` to output y: `[offset, factor]`. A nested
/// (winit) output is drawn upside down (`Flipped180`); scan-out buffers are
/// not.
// TODO(phase4-stage5): confirm the orientation on the DRM backend and for
// rotated outputs, which are not handled yet.
pub fn fb_y(transform: Transform, fb_height: f32) -> [f32; 2] {
    match transform {
        Transform::Flipped180 => [fb_height, -1.0],
        _ => [0.0, 1.0],
    }
}

smithay::backend::renderer::element::render_elements! {
    /// Everything sol-comp draws on an output, front to back.
    pub OutputElement<=GlesRenderer>;
    Window=Rounded,
    Shadow=PixelShaderElement,
    Wallpaper=MemoryRenderBufferRenderElement<GlesRenderer>,
}

/// The shadow element of a window, kept between frames so its id is
/// stable and only real changes count as damage.
#[derive(Default)]
struct ShadowCache(RefCell<Option<PixelShaderElement>>);

fn is_focused(window: &Window) -> bool {
    window.toplevel().is_some_and(|t| {
        t.current_state()
            .states
            .contains(xdg_toplevel::State::Activated)
    })
}

/// Where the shadow element goes and what it draws, in logical pixels
/// relative to the output: (area, window inside area, caster inside area).
pub fn shadow_layout(
    window: Rectangle<f64, Logical>,
    style: ShadowStyle,
) -> (Rectangle<f64, Logical>, [f64; 4], [f64; 4]) {
    let extent = style.sigma * 3.0;
    let area = Rectangle::new(
        (
            window.loc.x - extent,
            window.loc.y - extent + style.offset_y.min(0.0),
        )
            .into(),
        (
            window.size.w + 2.0 * extent,
            window.size.h + 2.0 * extent + style.offset_y.abs(),
        )
            .into(),
    );
    let win = [
        window.loc.x - area.loc.x,
        window.loc.y - area.loc.y,
        window.size.w,
        window.size.h,
    ];
    let caster = [win[0], win[1] + style.offset_y, win[2], win[3]];
    (area, win, caster)
}

/// Builds the output's elements, front to back: each window (its rounded
/// surfaces, then its shadow) from the top of the stack down, then the
/// wallpaper.
pub fn output_elements(
    renderer: &mut GlesRenderer,
    shaders: &Shaders,
    space: &Space<Window>,
    wallpaper: Option<&Wallpaper>,
    output_size: Size<i32, Logical>,
    scale: f64,
) -> Vec<OutputElement> {
    let mut out = Vec::new();
    let s = Scale::from(scale);
    #[allow(clippy::cast_possible_truncation)]
    let fb_height = (f64::from(output_size.h) * scale) as f32;
    for window in space.elements().rev() {
        let Some(geo) = space.element_geometry(window) else {
            continue;
        };
        let geo_f = geo.to_f64();
        let render_loc = (geo.loc - window.geometry().loc).to_physical_precise_round(s);
        let surfaces: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
            window.render_elements(renderer, render_loc, s, 1.0);
        #[allow(clippy::cast_possible_truncation)]
        let radius = (WINDOW_RADIUS * scale) as f32;
        let geo_px = geo_f.to_physical(s);
        out.extend(surfaces.into_iter().map(|inner| {
            OutputElement::Window(Rounded {
                inner,
                program: shaders.rounded.clone(),
                geo: geo_px,
                radius,
                fb_height,
            })
        }));
        out.push(OutputElement::Shadow(shadow_element(
            window, geo_f, shaders,
        )));
    }
    if let Some(wallpaper) = wallpaper
        && let Ok(e) = wallpaper_element(renderer, wallpaper, output_size)
    {
        out.push(OutputElement::Wallpaper(e));
    }
    out
}

fn shadow_element(
    window: &Window,
    geo: Rectangle<f64, Logical>,
    shaders: &Shaders,
) -> PixelShaderElement {
    let style = if is_focused(window) {
        SHADOW_FOCUSED
    } else {
        SHADOW_UNFOCUSED
    };
    let (area, win, caster) = shadow_layout(geo, style);
    #[allow(clippy::cast_possible_truncation)]
    let f = |v: [f64; 4]| [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
    #[allow(clippy::cast_possible_truncation)]
    let uniforms = vec![
        Uniform::new("window", f(win)),
        Uniform::new("caster", f(caster)),
        Uniform::new("radius", WINDOW_RADIUS as f32),
        Uniform::new("sigma", style.sigma as f32),
        Uniform::new("color", [0.0f32, 0.0, 0.0, style.opacity]),
    ];
    let area_i = area.to_i32_round();
    let cache = window.user_data().get_or_insert(ShadowCache::default);
    let mut slot = cache.0.borrow_mut();
    if let Some(e) = slot.as_mut() {
        e.resize(area_i, None);
        e.update_uniforms(uniforms);
        return e.clone();
    }
    let e = PixelShaderElement::new(
        shaders.shadow.clone(),
        area_i,
        None,
        1.0,
        uniforms,
        Kind::Unspecified,
    );
    *slot = Some(e.clone());
    e
}

/// The part of a `src` image that covers a `dst` area without distortion
/// (centred crop, like CSS `object-fit: cover`), as (x, y, w, h).
pub fn cover(src: (f64, f64), dst: (f64, f64)) -> (f64, f64, f64, f64) {
    let (sw, sh) = src;
    let (dw, dh) = dst;
    if sw <= 0.0 || sh <= 0.0 || dw <= 0.0 || dh <= 0.0 {
        return (0.0, 0.0, sw.max(0.0), sh.max(0.0));
    }
    let scale = (dw / sw).max(dh / sh);
    let (cw, ch) = (dw / scale, dh / scale);
    ((sw - cw) / 2.0, (sh - ch) / 2.0, cw, ch)
}

fn wallpaper_element(
    renderer: &mut GlesRenderer,
    wallpaper: &Wallpaper,
    output_size: Size<i32, Logical>,
) -> Result<MemoryRenderBufferRenderElement<GlesRenderer>, GlesError> {
    let (bw, bh) = wallpaper.size;
    let (x, y, w, h) = cover(
        (f64::from(bw), f64::from(bh)),
        (f64::from(output_size.w), f64::from(output_size.h)),
    );
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        (0.0, 0.0),
        &wallpaper.buffer,
        None,
        Some(Rectangle::new((x, y).into(), (w, h).into())),
        Some(output_size),
        Kind::Unspecified,
    )
}

/// A decoded wallpaper and its size in pixels.
#[derive(Debug)]
pub struct Wallpaper {
    pub buffer: MemoryRenderBuffer,
    pub size: (i32, i32),
}

/// Loads a PNG wallpaper into a memory buffer (8-bit RGB or RGBA).
///
/// # Errors
/// The file is missing, not a PNG, or in a layout we do not read.
pub fn load_wallpaper(path: &Path) -> Result<Wallpaper, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, h) = (info.width, info.height);
    // Memory buffers take little-endian ARGB, i.e. B, G, R, A bytes.
    let mut argb = Vec::with_capacity((w * h * 4) as usize);
    match info.color_type {
        png::ColorType::Rgba => {
            for px in buf[..info.buffer_size()].as_chunks::<4>().0 {
                argb.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        png::ColorType::Rgb => {
            for px in buf[..info.buffer_size()].as_chunks::<3>().0 {
                argb.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
        }
        other => {
            return Err(format!(
                "{}: unsupported PNG colour type {other:?}",
                path.display()
            ));
        }
    }
    let (wi, hi) = (
        i32::try_from(w).map_err(|_| "wallpaper too wide")?,
        i32::try_from(h).map_err(|_| "wallpaper too tall")?,
    );
    Ok(Wallpaper {
        buffer: MemoryRenderBuffer::from_slice(
            &argb,
            Fourcc::Argb8888,
            (wi, hi),
            1,
            Transform::Normal,
            None,
        ),
        size: (wi, hi),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_crops_the_long_side_and_centres() {
        // A 16:9 image on a 16:10 screen loses some width.
        let (x, y, w, h) = cover((1920.0, 1080.0), (1280.0, 800.0));
        assert!((h - 1080.0).abs() < 1e-9 && (w - 1728.0).abs() < 1e-9);
        assert!((x - 96.0).abs() < 1e-9 && y.abs() < 1e-9);
        // The same aspect: everything.
        assert_eq!(
            cover((2560.0, 1440.0), (1280.0, 720.0)),
            (0.0, 0.0, 2560.0, 1440.0)
        );
        // Degenerate sizes do not divide by zero.
        assert_eq!(cover((0.0, 10.0), (100.0, 100.0)), (0.0, 0.0, 0.0, 10.0));
    }

    #[test]
    // The layout is plain sums of exactly representable values.
    #[allow(clippy::float_cmp)]
    fn shadow_area_surrounds_the_window_and_the_caster_is_offset() {
        let w = Rectangle::new((100.0, 200.0).into(), (400.0, 300.0).into());
        let (area, win, caster) = shadow_layout(w, SHADOW_FOCUSED);
        let e = SHADOW_FOCUSED.sigma * 3.0;
        assert_eq!(area.loc, (100.0 - e, 200.0 - e).into());
        assert_eq!(
            area.size,
            (400.0 + 2.0 * e, 300.0 + 2.0 * e + SHADOW_FOCUSED.offset_y).into()
        );
        assert_eq!(win, [e, e, 400.0, 300.0]);
        assert_eq!(caster, [e, e + SHADOW_FOCUSED.offset_y, 400.0, 300.0]);
        // The caster stays inside the area, so the blur is never cut off
        // on the side it falls.
        assert!(caster[1] + caster[3] + e <= area.size.h + 1e-9);
    }

    // The focused window casts the stronger shadow (checked at compile time).
    const _: () = assert!(
        SHADOW_FOCUSED.sigma > SHADOW_UNFOCUSED.sigma
            && SHADOW_FOCUSED.opacity > SHADOW_UNFOCUSED.opacity
            && SHADOW_FOCUSED.offset_y > SHADOW_UNFOCUSED.offset_y
    );
}
