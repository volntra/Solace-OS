//! Our look on top of Smithay's GLES renderer: rounded window corners,
//! soft shadows tied to focus, and the wallpaper. Everything is drawn
//! through render elements, so the output damage tracker redraws only
//! what changed.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::backdrop::{self, Backdrop, BlurShaders, Clip};
use crate::blur::surface_blur;
use crate::text::{Fonts, Weight};
use crate::wm::Overlay;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Frame,
            element::{
                AsRenderElements, Element, Id, Kind, RenderElement, UnderlyingStorage,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::WaylandSurfaceRenderElement,
                utils::RescaleRenderElement,
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
// radii (top-left, top-right, bottom-right, bottom-left). gl_FragCoord is
// in framebuffer pixels; output y is fb_y.x + fb_y.y * gl_FragCoord.y (the
// framebuffer may be upside down).
uniform vec4 geo;
uniform vec4 radii;
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
    vec2 center = geo.xy + geo.zw * 0.5;
    float radius = p.x < center.x ? (p.y < center.y ? radii.x : radii.w)
                                  : (p.y < center.y ? radii.y : radii.z);
    float d = rounded_rect_sdf(p, center, geo.zw * 0.5, radius);
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

/// A solid rounded rectangle (titlebars, traffic lights, overlay boxes),
/// optionally with a border ring of another colour. Colours premultiplied.
const SHAPE_FRAG: &str = r"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
uniform vec4 color;
uniform vec4 radii;
uniform vec4 border_color;
uniform float border;

float rounded_rect_sdf(vec2 p, vec2 center, vec2 half_size, float r) {
    vec2 q = abs(p - center) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 p = v_coords * size;
    vec2 center = size * 0.5;
    float r = p.x < center.x ? (p.y < center.y ? radii.x : radii.w)
                             : (p.y < center.y ? radii.y : radii.z);
    float d = rounded_rect_sdf(p, center, center, r);
    float coverage = clamp(0.5 - d, 0.0, 1.0);
    vec4 c = color;
    if (border > 0.0) {
        float inside = clamp(0.5 - (d + border), 0.0, 1.0);
        c = mix(border_color, color, inside);
    }
    gl_FragColor = c * coverage * alpha;
}
";

/// The compiled shaders (one set per renderer).
#[derive(Debug, Clone)]
pub struct Shaders {
    rounded: GlesTexProgram,
    shadow: GlesPixelProgram,
    shape: GlesPixelProgram,
    blur: BlurShaders,
    /// Inter, for titles and overlay text.
    pub fonts: Fonts,
    /// The traffic lights' hover glyphs (close, minimize, zoom), made on
    /// first use.
    glyphs: RefCell<Option<[Rendered; 3]>>,
    /// The window manager's overlay rectangles, by position in the list.
    overlay_shapes: ShapeCache,
    /// Overlay text by what it was made from; kept while drawn.
    overlay_texts: RefCell<std::collections::HashMap<TextKey, Option<Rendered>>>,
}

/// What a line of overlay text was rendered from: text, size, colour,
/// room.
type TextKey = (String, i32, u32, i32);

impl Shaders {
    /// # Errors
    /// The GLSL does not compile on this driver.
    pub fn compile(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let rounded = renderer.compile_custom_texture_shader(
            ROUNDED_FRAG,
            &[
                UniformName::new("geo", UniformType::_4f),
                UniformName::new("radii", UniformType::_4f),
                UniformName::new("fb_y", UniformType::_2f),
            ],
        )?;
        let shape = renderer.compile_custom_pixel_shader(
            SHAPE_FRAG,
            &[
                UniformName::new("color", UniformType::_4f),
                UniformName::new("radii", UniformType::_4f),
                UniformName::new("border_color", UniformType::_4f),
                UniformName::new("border", UniformType::_1f),
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
        let blur = BlurShaders::compile(renderer)?;
        Ok(Shaders {
            rounded,
            shadow,
            shape,
            blur,
            fonts: Fonts::load(),
            glyphs: RefCell::new(None),
            overlay_shapes: ShapeCache::default(),
            overlay_texts: RefCell::default(),
        })
    }
}

/// A window surface element clipped to the window's rounded rectangle.
#[derive(Debug)]
pub struct Rounded {
    /// The surface, scaled about the window's drawn origin (1.0 unless the
    /// window manager shrinks it: Overview, minimize).
    inner: RescaleRenderElement<WaylandSurfaceRenderElement<GlesRenderer>>,
    program: GlesTexProgram,
    /// The window's rounded rectangle, relative to this element's
    /// geometry (so it follows the element when drawn offscreen).
    clip: Rectangle<f64, Physical>,
    radii: [f32; 4],
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
        let uniforms = clip_uniforms(
            frame.transformation(),
            dst.loc,
            self.clip,
            self.radii,
            self.fb_height,
        );
        frame.override_default_tex_program(self.program.clone(), uniforms.into());
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

/// The rounded-clip shader's uniforms for an element drawn at `dst`
/// whose clip rectangle is `clip` relative to it.
pub fn clip_uniforms(
    transform: Transform,
    dst: Point<i32, Physical>,
    clip: Rectangle<f64, Physical>,
    radii: [f32; 4],
    fb_height: f32,
) -> [Uniform<'static>; 3] {
    let loc = clip.loc + dst.to_f64();
    #[allow(clippy::cast_possible_truncation)]
    let geo = [
        loc.x as f32,
        loc.y as f32,
        clip.size.w as f32,
        clip.size.h as f32,
    ];
    [
        Uniform::new("geo", geo),
        Uniform::new("radii", radii),
        Uniform::new("fb_y", fb_y(transform, fb_height)),
    ]
}

smithay::backend::renderer::element::render_elements! {
    /// Everything fern-comp draws on an output, front to back.
    pub OutputElement<=GlesRenderer>;
    Window=Rounded,
    Backdrop=Backdrop,
    Shadow=PixelShaderElement,
    /// The wallpaper, the pointer.
    Image=MemoryRenderBufferRenderElement<GlesRenderer>,
}

/// The shadow element of a window, kept between frames so its id is
/// stable and only real changes count as damage; with the window size
/// and style its uniforms were made for (they do not depend on position).
#[derive(Default)]
struct ShadowCache(RefCell<Option<CachedShadow>>);

type CachedShadow = (PixelShaderElement, Size<f64, Logical>, ShadowStyle);

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

/// Builds the elements of the output showing `area` of the space, front
/// to back: each window (its rounded surfaces, its blurred backdrop if it
/// asked for one, its shadow) from the top of the stack down, then the
/// wallpaper. Element positions are relative to the output.
/// How a window is drawn relative to where it is in the space: the
/// window manager's springs move, scale and fade it (`crate::wm`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Visual {
    /// Offset of the drawn frame from the window's place, logical pixels.
    pub dx: f64,
    pub dy: f64,
    /// 1.0: actual size.
    pub scale: f64,
    pub alpha: f32,
    /// Height of our titlebar above the content (0: none), logical px.
    pub titlebar: i32,
    /// The pointer is over the traffic lights: show their glyphs.
    pub hover: bool,
    /// Fullscreen: square corners and no shadow.
    pub square: bool,
}

impl Visual {
    pub const IDENTITY: Visual = Visual {
        dx: 0.0,
        dy: 0.0,
        scale: 1.0,
        alpha: 1.0,
        titlebar: 0,
        hover: false,
        square: false,
    };
}

impl Default for Visual {
    fn default() -> Self {
        Visual::IDENTITY
    }
}

// Everything an output shows, from its parts: long, and each argument
// is a different source.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn output_elements(
    renderer: &mut GlesRenderer,
    shaders: &Shaders,
    space: &Space<Window>,
    wallpaper: Option<&Wallpaper>,
    area: Rectangle<i32, Logical>,
    scale: f64,
    visual: &dyn Fn(&Window) -> Visual,
    overlay: (&[Overlay], Option<Window>),
) -> Vec<OutputElement> {
    let (overlay, beneath) = overlay;
    let mut overlay_drawn = false;
    let output_size = area.size;
    // Built from the bottom up, so that everything behind a blurred
    // window is known when its backdrop is made.
    let mut out: VecDeque<OutputElement> = VecDeque::new();
    let s = Scale::from(scale);
    let output_px = output_size.to_physical_precise_round(s);
    #[allow(clippy::cast_precision_loss)]
    let fb_height = output_px.h as f32;
    if let Some(wallpaper) = wallpaper
        && let Ok(e) = wallpaper_element(renderer, wallpaper, output_size)
    {
        out.push_front(OutputElement::Image(e));
    }
    for window in space.elements() {
        if beneath.as_ref() == Some(window) {
            overlay_elements(renderer, shaders, overlay, area, &mut out);
            overlay_drawn = true;
        }
        let Some(geo) = space.element_geometry(window) else {
            continue;
        };
        let v = visual(window);
        if v.alpha <= 0.0 {
            continue;
        }
        // From here on, in output coordinates, where the window is drawn.
        let geo = Rectangle::new(geo.loc - area.loc, geo.size);
        let geo_f: Rectangle<f64, Logical> = Rectangle::new(
            (f64::from(geo.loc.x) + v.dx, f64::from(geo.loc.y) + v.dy).into(),
            (
                f64::from(geo.size.w) * v.scale,
                f64::from(geo.size.h) * v.scale,
            )
                .into(),
        );
        let geo_px = geo_f.to_physical(s);
        #[allow(clippy::cast_possible_truncation)]
        let radius = if v.square {
            0.0
        } else {
            (WINDOW_RADIUS * scale * v.scale) as f32
        };
        // With our titlebar on top, the frame is titlebar plus content and
        // only the content's bottom corners are rounded.
        let titlebar = f64::from(v.titlebar) * v.scale;
        let frame_f: Rectangle<f64, Logical> = Rectangle::new(
            (geo_f.loc.x, geo_f.loc.y - titlebar).into(),
            (geo_f.size.w, geo_f.size.h + titlebar).into(),
        );
        let radii = if v.titlebar > 0 {
            [0.0, 0.0, radius, radius]
        } else {
            [radius; 4]
        };
        let origin_f = geo_f.loc - window.geometry().loc.to_f64().upscale(v.scale);
        let moved = v.dx != 0.0 || v.dy != 0.0;
        // Blur behind a window that is moving or scaled waits until it
        // settles (the cache is keyed on where the window is).
        let blurred = if moved || (v.scale - 1.0).abs() > 1e-6 {
            Vec::new()
        } else {
            blur_rects(window, geo.loc - window.geometry().loc, geo, s)
        };
        // The backdrop is made before the window's own shadow is added:
        // a window does not see its shadow through itself.
        let mut backdrops = Vec::new();
        if !blurred.is_empty() {
            let clip = Clip {
                window: geo_px,
                radii,
                fb_height,
            };
            match backdrop::backdrops(
                renderer,
                &shaders.blur,
                &shaders.rounded,
                window,
                &blurred,
                clip,
                out.make_contiguous(),
                output_px,
                s,
            ) {
                Ok(b) => backdrops = b,
                Err(e) => tracing::warn!("blur: {e:?}"),
            }
        }
        if !v.square {
            out.push_front(OutputElement::Shadow(shadow_element(
                window, frame_f, shaders, v.scale, v.alpha,
            )));
        }
        for b in backdrops {
            out.push_front(OutputElement::Backdrop(b));
        }
        if v.titlebar > 0 {
            decorations(renderer, shaders, window, frame_f, &v, &mut out);
        }
        let origin_px = origin_f.to_physical_precise_round(s);
        let surfaces: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
            window.render_elements(renderer, origin_px, s, v.alpha);
        for surface in surfaces.into_iter().rev() {
            let inner = RescaleRenderElement::from_element(surface, origin_px, v.scale);
            let at = inner.geometry(s).loc.to_f64();
            out.push_front(OutputElement::Window(Rounded {
                inner,
                program: shaders.rounded.clone(),
                clip: Rectangle::new(geo_px.loc - at, geo_px.size),
                radii,
                fb_height,
            }));
        }
    }
    if !overlay_drawn {
        overlay_elements(renderer, shaders, overlay, area, &mut out);
    }
    out.into()
}

/// `0xRRGGBBAA` as straight-alpha floats.
fn unpack_rgba(rgba: u32) -> [f32; 4] {
    rgba.to_be_bytes().map(|v| f32::from(v) / 255.0)
}

/// The window manager's overlay, above every window, in list order (the
/// first is lowest). Coordinates are global; `area` is this output.
fn overlay_elements(
    renderer: &mut GlesRenderer,
    shaders: &Shaders,
    overlay: &[Overlay],
    area: Rectangle<i32, Logical>,
    out: &mut VecDeque<OutputElement>,
) {
    let origin = area.loc.to_f64();
    let mut texts = shaders.overlay_texts.borrow_mut();
    let mut used = std::collections::HashMap::new();
    for (i, item) in overlay.iter().enumerate() {
        match item {
            Overlay::Rect {
                x,
                y,
                w,
                h,
                radius,
                rgba,
            } => {
                let rect = Rectangle::<f64, Logical>::new(
                    (f64::from(*x) - origin.x, f64::from(*y) - origin.y).into(),
                    (f64::from(*w), f64::from(*h)).into(),
                );
                #[allow(clippy::cast_precision_loss)]
                let r = (*radius).max(0) as f32;
                #[allow(clippy::cast_possible_truncation)]
                let slot = i as u32;
                out.push_front(OutputElement::Shadow(cached_shape(
                    &shaders.overlay_shapes,
                    slot,
                    shaders,
                    rect,
                    premul(unpack_rgba(*rgba), 1.0),
                    [r; 4],
                    0.0,
                    [0.0; 4],
                )));
            }
            Overlay::Text {
                x,
                y,
                width,
                size,
                rgba,
                align,
                text,
            } => {
                let key: TextKey = (text.clone(), *size, *rgba, *width);
                let made = texts.remove(&key).unwrap_or_else(|| {
                    #[allow(clippy::cast_precision_loss)]
                    let line = shaders.fonts.line(
                        text,
                        *size as f32,
                        Weight::SemiBold,
                        unpack_rgba(*rgba),
                        *width as f32,
                    );
                    line.map(|l| {
                        (
                            MemoryRenderBuffer::from_slice(
                                &l.pixels,
                                Fourcc::Argb8888,
                                (l.width, l.height),
                                1,
                                Transform::Normal,
                                None,
                            ),
                            (l.width, l.height),
                        )
                    })
                });
                if let Some((buffer, (tw, _))) = &made {
                    let left = match align {
                        1 => (width - tw) / 2,
                        2 => width - tw,
                        _ => 0,
                    };
                    let at = Point::<f64, Logical>::from((
                        f64::from(x + left.max(0)) - origin.x,
                        f64::from(*y) - origin.y,
                    ));
                    if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(
                        renderer,
                        at.to_physical(1.0),
                        buffer,
                        None,
                        None,
                        None,
                        Kind::Unspecified,
                    ) {
                        out.push_front(OutputElement::Image(e));
                    }
                }
                used.insert(key, made);
            }
            // Drawn from stage 7 on (Overview); nothing uses them yet.
            Overlay::Wallpaper { .. } | Overlay::Mirror { .. } => {}
        }
    }
    // Text no longer shown is dropped; the rest is kept for next frame.
    *texts = used;
}

/// Text rendered into a buffer, with its size in pixels.
type Rendered = (MemoryRenderBuffer, (i32, i32));

/// Titlebar height, logical pixels (HIG: Window).
pub const TITLEBAR_HEIGHT: i32 = 28;
/// The traffic lights: diameter, the first one's centre from the frame's
/// left edge, the distance between centres; all on the titlebar's middle.
pub const BUTTON_DIAMETER: f64 = 12.0;
pub const BUTTON_FIRST_X: f64 = 20.0;
pub const BUTTON_STEP: f64 = 20.0;
/// The title keeps clear of the buttons on both sides.
const TITLE_INSET: f64 = 76.0;
const TITLE_SIZE: f32 = 13.0;

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}
const TITLEBAR_ACTIVE: [f32; 4] = rgb(0xEA, 0xEC, 0xE7);
const TITLEBAR_INACTIVE: [f32; 4] = rgb(0xF3, 0xF4, 0xF1);
const SEPARATOR: [f32; 4] = [0.0, 0.0, 0.0, 0.09];
/// Close, minimize, zoom (our own colours, not anyone else's).
const LIGHTS: [[f32; 4]; 3] = [
    rgb(0xEE, 0x5D, 0x50),
    rgb(0xF2, 0xB5, 0x3A),
    rgb(0x3D, 0xB8, 0x62),
];
const LIGHT_OFF: [f32; 4] = rgb(0xD3, 0xD6, 0xD0);
const TITLE_ACTIVE: [f32; 4] = [0.063, 0.082, 0.071, 0.85];
const TITLE_INACTIVE: [f32; 4] = [0.063, 0.082, 0.071, 0.40];

/// Premultiplies `c` and fades it by `alpha`.
fn premul(c: [f32; 4], alpha: f32) -> [f32; 4] {
    let a = c[3] * alpha;
    [c[0] * a, c[1] * a, c[2] * a, a]
}

/// A darker ring for a traffic light (premultiplied).
fn ring(c: [f32; 4], alpha: f32) -> [f32; 4] {
    premul([c[0] * 0.82, c[1] * 0.82, c[2] * 0.82, c[3]], alpha)
}

// One element after another down the titlebar: long, but a list.
#[allow(clippy::too_many_lines)]
/// The titlebar of `window`, drawn at the top of `frame`: background,
/// separator, title, and the traffic lights (with glyphs on hover; grey
/// when the window is inactive and not hovered).
fn decorations(
    renderer: &mut GlesRenderer,
    shaders: &Shaders,
    window: &Window,
    frame: Rectangle<f64, Logical>,
    vis: &Visual,
    out: &mut VecDeque<OutputElement>,
) {
    let zoom = vis.scale;
    let active = is_focused(window);
    let bar = Rectangle::<f64, Logical>::new(
        frame.loc,
        (frame.size.w, f64::from(TITLEBAR_HEIGHT) * zoom).into(),
    );
    #[allow(clippy::cast_possible_truncation)]
    let r = (WINDOW_RADIUS * zoom) as f32;
    let bg = if active {
        TITLEBAR_ACTIVE
    } else {
        TITLEBAR_INACTIVE
    };
    let cache = window.user_data().get_or_insert(ShapeCache::default);
    let shape = |slot, rect, color, radii, border: f32, border_color| {
        OutputElement::Shadow(cached_shape(
            cache,
            slot,
            shaders,
            rect,
            color,
            radii,
            border,
            border_color,
        ))
    };
    out.push_front(shape(
        0,
        bar,
        premul(bg, vis.alpha),
        [r, r, 0.0, 0.0],
        0.0,
        [0.0; 4],
    ));
    let line = Rectangle::new(
        (bar.loc.x, bar.loc.y + bar.size.h - 1.0).into(),
        (bar.size.w, 1.0).into(),
    );
    out.push_front(shape(
        1,
        line,
        premul(SEPARATOR, vis.alpha),
        [0.0; 4],
        0.0,
        [0.0; 4],
    ));
    // The title, centred, clear of the buttons.
    if let Some((buffer, (tw, th))) = title_buffer(shaders, window, active, frame.size.w / zoom) {
        let (tw, th) = (f64::from(tw) * zoom, f64::from(th) * zoom);
        let x = (bar.loc.x + (bar.size.w - tw) / 2.0).max(bar.loc.x + TITLE_INSET * zoom);
        let y = bar.loc.y + (bar.size.h - th) / 2.0;
        #[allow(clippy::cast_possible_truncation)]
        let size = Size::<i32, Logical>::from((tw.round() as i32, th.round() as i32));
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, Logical>::from((x, y)).to_physical(1.0),
            &buffer,
            Some(vis.alpha),
            None,
            Some(size),
            Kind::Unspecified,
        ) {
            out.push_front(OutputElement::Image(e));
        }
    }
    // The traffic lights.
    let d = BUTTON_DIAMETER * zoom;
    #[allow(clippy::cast_possible_truncation)]
    let half = (d / 2.0) as f32;
    let glyphs = vis.hover.then(|| traffic_light_glyphs(shaders)).flatten();
    for (i, colour) in LIGHTS.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let cx = bar.loc.x + (BUTTON_FIRST_X + BUTTON_STEP * i as f64) * zoom;
        let cy = bar.loc.y + bar.size.h / 2.0;
        let rect = Rectangle::new((cx - d / 2.0, cy - d / 2.0).into(), (d, d).into());
        let c = if active || vis.hover {
            *colour
        } else {
            LIGHT_OFF
        };
        #[allow(clippy::cast_possible_truncation)]
        let slot = 2 + i as u32;
        out.push_front(shape(
            slot,
            rect,
            premul(c, vis.alpha),
            [half; 4],
            0.5,
            ring(c, vis.alpha),
        ));
        if let Some(glyphs) = &glyphs
            && let Some(g) = glyphs.get(i)
        {
            let (gw, gh) = (f64::from(g.1.0) * zoom, f64::from(g.1.1) * zoom);
            #[allow(clippy::cast_possible_truncation)]
            let size = Size::<i32, Logical>::from((gw.round() as i32, gh.round() as i32));
            if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                Point::<f64, Logical>::from((cx - gw / 2.0, cy - gh / 2.0)).to_physical(1.0),
                &g.0,
                Some(vis.alpha),
                None,
                Some(size),
                Kind::Unspecified,
            ) {
                out.push_front(OutputElement::Image(e));
            }
        }
    }
}

/// Shapes already made for a window, by slot, so a titlebar that does not
/// change is the same element (no damage) frame after frame.
#[derive(Debug, Clone, Default)]
struct ShapeCache(RefCell<std::collections::HashMap<u32, (PixelShaderElement, ShapeKey)>>);

#[derive(Debug, Clone, Copy, PartialEq)]
struct ShapeKey {
    rect: Rectangle<i32, Logical>,
    params: [u32; 13],
}

#[allow(clippy::too_many_arguments)]
fn cached_shape(
    cache: &ShapeCache,
    slot: u32,
    shaders: &Shaders,
    rect: Rectangle<f64, Logical>,
    color: [f32; 4],
    radii: [f32; 4],
    border: f32,
    border_color: [f32; 4],
) -> PixelShaderElement {
    let rect = rect.to_i32_round();
    let mut params = [0u32; 13];
    for (p, v) in params.iter_mut().zip(
        color
            .iter()
            .chain(&radii)
            .chain(&border_color)
            .chain([&border]),
    ) {
        *p = v.to_bits();
    }
    let key = ShapeKey { rect, params };
    let uniforms = || {
        vec![
            Uniform::new("color", color),
            Uniform::new("radii", radii),
            Uniform::new("border_color", border_color),
            Uniform::new("border", border),
        ]
    };
    let mut map = cache.0.borrow_mut();
    if let Some((e, old)) = map.get_mut(&slot) {
        if old.rect != key.rect {
            e.resize(key.rect, None);
        }
        if old.params != key.params {
            e.update_uniforms(uniforms());
        }
        *old = key;
        return e.clone();
    }
    let e = PixelShaderElement::new(
        shaders.shape.clone(),
        key.rect,
        None,
        1.0,
        uniforms(),
        Kind::Unspecified,
    );
    map.insert(slot, (e.clone(), key));
    e
}

/// A window's rendered title, remade only when it or its state changes.
#[derive(Default)]
struct TitleCache(RefCell<Option<TitleEntry>>);

/// What a title was rendered from (text, active, room) and the result.
type TitleEntry = (String, bool, i32, Option<Rendered>);

fn title_buffer(
    shaders: &Shaders,
    window: &Window,
    active: bool,
    frame_width: f64,
) -> Option<Rendered> {
    let title = window.toplevel().map(|t| {
        smithay::wayland::compositor::with_states(t.wl_surface(), |states| {
            states
                .data_map
                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok())
                .and_then(|d| d.title.clone())
                .unwrap_or_default()
        })
    })?;
    #[allow(clippy::cast_possible_truncation)]
    let max = (frame_width - 2.0 * TITLE_INSET).max(0.0) as i32;
    let cache = window.user_data().get_or_insert(TitleCache::default);
    let mut slot = cache.0.borrow_mut();
    if let Some((t, a, m, buf)) = slot.as_ref()
        && *t == title
        && *a == active
        && *m == max
    {
        return buf.clone();
    }
    let colour = if active { TITLE_ACTIVE } else { TITLE_INACTIVE };
    #[allow(clippy::cast_precision_loss)]
    let line = shaders
        .fonts
        .line(&title, TITLE_SIZE, Weight::SemiBold, colour, max as f32);
    let buf = line.map(|l| {
        (
            MemoryRenderBuffer::from_slice(
                &l.pixels,
                Fourcc::Argb8888,
                (l.width, l.height),
                1,
                Transform::Normal,
                None,
            ),
            (l.width, l.height),
        )
    });
    *slot = Some((title, active, max, buf.clone()));
    buf
}

/// The glyphs shown on the traffic lights while hovered (close,
/// minimize, zoom) and their sizes; made once.
fn traffic_light_glyphs(shaders: &Shaders) -> Option<Vec<Rendered>> {
    let mut slot = shaders.glyphs.borrow_mut();
    if slot.is_none() {
        let make = |c: &str| {
            shaders
                .fonts
                .line(c, 11.0, Weight::SemiBold, [0.0, 0.0, 0.0, 0.6], 20.0)
                .map(|l| {
                    (
                        MemoryRenderBuffer::from_slice(
                            &l.pixels,
                            Fourcc::Argb8888,
                            (l.width, l.height),
                            1,
                            Transform::Normal,
                            None,
                        ),
                        (l.width, l.height),
                    )
                })
        };
        if let (Some(a), Some(b), Some(c)) = (make("\u{00D7}"), make("\u{2212}"), make("+")) {
            *slot = Some([a, b, c]);
        }
    }
    slot.as_ref().map(|g| g.to_vec())
}

/// The blurred parts of a window's main surface, in output pixels,
/// within the window.
fn blur_rects(
    window: &Window,
    origin: Point<i32, Logical>,
    geo: Rectangle<i32, Logical>,
    scale: Scale<f64>,
) -> Vec<Rectangle<i32, Physical>> {
    let Some(toplevel) = window.toplevel() else {
        return Vec::new();
    };
    let surface = toplevel.wl_surface();
    let size = window.bbox().size;
    surface_blur(surface)
        .rects(size)
        .into_iter()
        .filter_map(|r| Rectangle::new(r.loc + origin, r.size).intersection(geo))
        .map(|r| r.to_physical_precise_round(scale))
        .collect()
}

fn shadow_element(
    window: &Window,
    geo: Rectangle<f64, Logical>,
    shaders: &Shaders,
    zoom: f64,
    alpha: f32,
) -> PixelShaderElement {
    let base = if is_focused(window) {
        SHADOW_FOCUSED
    } else {
        SHADOW_UNFOCUSED
    };
    // A scaled window casts a scaled shadow; a fading one a fainter one.
    let style = ShadowStyle {
        sigma: base.sigma * zoom,
        offset_y: base.offset_y * zoom,
        opacity: base.opacity * alpha,
    };
    let (area, win, caster) = shadow_layout(geo, style);
    #[allow(clippy::cast_possible_truncation)]
    let f = |v: [f64; 4]| [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
    #[allow(clippy::cast_possible_truncation)]
    let uniforms = vec![
        Uniform::new("window", f(win)),
        Uniform::new("caster", f(caster)),
        Uniform::new("radius", (WINDOW_RADIUS * zoom) as f32),
        Uniform::new("sigma", style.sigma as f32),
        Uniform::new("color", [0.0f32, 0.0, 0.0, style.opacity]),
    ];
    let area_i = area.to_i32_round();
    let cache = window.user_data().get_or_insert(ShadowCache::default);
    let mut slot = cache.0.borrow_mut();
    if let Some((e, size, last)) = slot.as_mut() {
        // Both only count as a change (damage) when something differs.
        e.resize(area_i, None);
        if *size != geo.size || *last != style {
            e.update_uniforms(uniforms);
            (*size, *last) = (geo.size, style);
        }
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
    *slot = Some((e.clone(), geo.size, style));
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

impl Wallpaper {
    /// A wallpaper of one colour (RGB), e.g. for tests.
    pub fn solid([r, g, b]: [u8; 3]) -> Self {
        Wallpaper {
            buffer: MemoryRenderBuffer::from_slice(
                &[b, g, r, 255],
                Fourcc::Argb8888,
                (1, 1),
                1,
                Transform::Normal,
                None,
            ),
            size: (1, 1),
        }
    }
}

/// The wallpaper: `FERN_WALLPAPER`, the installed default, or the
/// development build's render (`just wallpapers`). Missing is not fatal.
pub fn default_wallpaper() -> Option<Wallpaper> {
    let candidates: Vec<PathBuf> = std::env::var_os("FERN_WALLPAPER")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            PathBuf::from("/usr/share/fern/wallpapers/grove.png"),
            PathBuf::from("build/wallpapers/grove.png"),
        ])
        .collect();
    for path in &candidates {
        if !path.exists() {
            continue;
        }
        match load_wallpaper(path) {
            Ok(w) => return Some(w),
            Err(e) => tracing::warn!("wallpaper: {e}"),
        }
    }
    tracing::info!("no wallpaper found; using the plain background");
    None
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
