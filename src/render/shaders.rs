//! GLSL shaders for everything pzwm draws itself: the canvas grid, window
//! frames (shadow, gradient border, titlebar, buttons), the snap preview, and
//! the rounded-corner clip applied to client surfaces.

use smithay::backend::renderer::{
    element::{
        Element, Id, Kind, RenderElement, UnderlyingStorage, surface::WaylandSurfaceRenderElement,
        utils::RescaleRenderElement,
    },
    gles::{
        GlesError, GlesPixelProgram, GlesRenderer, GlesTexProgram, Uniform, UniformName,
        UniformType,
    },
    utils::{CommitCounter, DamageSet, OpaqueRegions},
};
use smithay::utils::{Buffer, Physical, Point, Rectangle, Scale, Size, Transform};

use super::PzRenderer;

const PRECISION: &str = r#"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
"#;

const GRID_FRAG: &str = r#"
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
uniform vec2 u_cam;       // canvas point at the element's top-left
uniform float u_px;       // physical pixels per canvas unit
uniform float u_spacing;  // canvas units between grid points
uniform float u_fade;     // 0..1 visibility of the minor grid
uniform float u_style;    // 1 dots, 2 lines
uniform float u_dpi;      // output scale, keeps dots the same size on HiDPI
uniform vec4 u_color;

float grid_dist(vec2 p, float s) {
    vec2 c = mod(p, s);
    vec2 d = min(c, s - c) * u_px;
    return u_style < 1.5 ? length(d) : min(d.x, d.y);
}

void main() {
    vec2 p = u_cam + (v_coords * size) / u_px;
    float minor_r = u_style < 1.5 ? 1.05 * u_dpi : 0.5 * u_dpi;
    float major_r = u_style < 1.5 ? 1.6 * u_dpi : 0.75 * u_dpi;
    float minor = 1.0 - smoothstep(minor_r - 0.7, minor_r + 0.7, grid_dist(p, u_spacing));
    float major = 1.0 - smoothstep(major_r - 0.7, major_r + 0.7, grid_dist(p, u_spacing * 4.0));
    float a = max(minor * u_fade * 0.85, major) * u_color.a * alpha;
    gl_FragColor = vec4(u_color.rgb * a, a);
}
"#;

const FRAME_FRAG: &str = r#"
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
uniform vec4 u_frame;      // titlebar + content, element-local pixels
uniform float u_title;     // titlebar height in pixels, 0 when absent
uniform float u_radius;
uniform float u_border;
uniform float u_shadow;
uniform vec4 u_c1;         // border gradient start
uniform vec4 u_c2;         // border gradient end
uniform vec4 u_title_col;
uniform vec4 u_shadow_col;
uniform vec4 u_btn;        // x: buttons on, y: hovered (1 close, 2 max, 3 pin), z: pinned, w: maximized
uniform vec4 u_accent;
uniform vec4 u_max_col;    // maximize button
uniform float u_mode;      // 0 window frame, 1 snap preview

float box(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + r;
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

vec4 over(vec4 dst, vec4 src) {   // premultiplied "src over dst"
    return src + dst * (1.0 - src.a);
}

vec4 circle(vec2 p, vec2 c, float r, vec3 col, float on) {
    float d = length(p - c) - r;
    float a = (1.0 - smoothstep(-0.8, 0.8, d)) * on;
    return vec4(col * a, a);
}

void main() {
    vec2 p = v_coords * size;
    vec2 fc = u_frame.xy + u_frame.zw * 0.5;
    vec2 outer_half = u_frame.zw * 0.5 + vec2(u_border);
    float d_out = box(p - fc, outer_half, u_radius);
    vec4 col = vec4(0.0);

    if (u_mode > 0.5) {   // snap preview: soft glassy rounded rectangle
        float fill = 1.0 - smoothstep(-1.0, 1.0, d_out);
        float edge = fill * smoothstep(-u_border - 1.5, -u_border + 0.5, d_out);
        vec4 tint_col = mix(u_c1, u_c2, v_coords.x);
        col = vec4(tint_col.rgb, 1.0) * (fill * 0.16 + edge * 0.7);
        gl_FragColor = col * alpha;
        return;
    }

    // Shadow, slightly lowered for depth.
    if (u_shadow > 0.0) {
        float d_sh = box(p - fc - vec2(0.0, u_shadow * 0.18), outer_half, u_radius);
        float s = 1.0 - smoothstep(-u_shadow * 0.35, u_shadow, d_sh);
        float a = s * s * u_shadow_col.a;
        col = vec4(u_shadow_col.rgb * a, a);
    }

    // Border ring with a diagonal gradient.
    float inside_out = 1.0 - smoothstep(-0.6, 0.6, d_out);
    float d_in = box(p - fc, u_frame.zw * 0.5, max(u_radius - u_border, 0.0));
    float ring = inside_out * smoothstep(-1.2, 0.0, d_in);
    float t = clamp(dot(v_coords, vec2(0.6, 0.4)), 0.0, 1.0);
    vec4 bc = mix(u_c1, u_c2, t);
    col = over(col, vec4(bc.rgb, 1.0) * bc.a * ring);

    // Titlebar.
    float in_frame = 1.0 - smoothstep(-0.6, 0.6, d_in);
    float title_y = p.y - u_frame.y;
    if (u_title > 0.0 && title_y < u_title + 0.5) {
        float shade = 1.0 + 0.06 * (1.0 - title_y / u_title);
        vec4 tc = vec4(u_title_col.rgb * shade, 1.0) * u_title_col.a * in_frame;
        float sep = smoothstep(u_title - 1.2, u_title - 0.2, title_y) * 0.35 * in_frame;
        tc = over(tc, vec4(vec3(0.0), sep));
        col = over(col, tc);

        if (u_btn.x > 0.5) {
            float right = u_frame.x + u_frame.z;
            float cy = u_frame.y + u_title * 0.5;
            float r = u_title * 0.2;
            for (int i = 0; i < 3; i++) {
                float fi = float(i);
                vec2 c = vec2(right - u_title * 0.62 - fi * u_title * 0.72, cy);
                bool hov = abs(u_btn.y - (fi + 1.0)) < 0.5;
                vec3 bcol = i == 0 ? vec3(1.0, 0.37, 0.34)
                          : i == 1 ? u_max_col.rgb
                          : (u_btn.z > 0.5 ? u_accent.rgb : vec3(0.42, 0.45, 0.52));
                float grow = hov ? 1.25 : 1.0;
                col = over(col, circle(p, c, r * grow, bcol * (hov ? 1.15 : 0.92), in_frame));
                if (hov) {
                    float ring_d = abs(length(p - c) - r * 1.9);
                    float ra = (1.0 - smoothstep(0.4, 1.4, ring_d)) * 0.35;
                    col = over(col, vec4(bcol * ra, ra));
                }
            }
        }
    }

#if defined(DEBUG_FLAGS)
    if (tint == 1.0) col = vec4(0.0, 0.2, 0.0, 0.2) + col * 0.8;
#endif
    gl_FragColor = col * alpha;
}
"#;

/// Texture shader that cuts rounded corners out of client surfaces.
const CLIP_FRAG: &str = r#"
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
#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
uniform vec4 u_uv_to_px;   // xy scale, zw offset: output pixel = uv * xy + zw
uniform vec4 u_clip;       // rounded rect in output pixels
uniform vec2 u_radii;      // top, bottom corner radius

void main() {
    vec4 color = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
#endif
    vec2 px = v_coords * u_uv_to_px.xy + u_uv_to_px.zw;
    vec2 half_size = u_clip.zw * 0.5;
    vec2 q = px - (u_clip.xy + half_size);
    float r = q.y < 0.0 ? u_radii.x : u_radii.y;
    vec2 k = abs(q) - half_size + r;
    float d = min(max(k.x, k.y), 0.0) + length(max(k, 0.0)) - r;
    color *= alpha * (1.0 - smoothstep(-0.5, 0.5, d));
#if defined(DEBUG_FLAGS)
    if (tint == 1.0) color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    gl_FragColor = color;
}
"#;

/// Programs compiled for one GL context. Stored in the context's user data,
/// so every GPU gets its own copy.
#[derive(Clone)]
pub struct Shaders {
    pub grid: GlesPixelProgram,
    pub frame: GlesPixelProgram,
    pub clip: GlesTexProgram,
}

impl Shaders {
    pub fn get(renderer: &mut GlesRenderer) -> Option<Shaders> {
        if let Some(shaders) = renderer.egl_context().user_data().get::<Option<Shaders>>() {
            return shaders.clone();
        }
        let compiled = Self::compile(renderer)
            .inspect_err(|err| {
                tracing::error!("shader compilation failed, using plain rendering: {err}")
            })
            .ok();
        renderer
            .egl_context()
            .user_data()
            .insert_if_missing(|| compiled.clone());
        compiled
    }

    fn compile(renderer: &mut GlesRenderer) -> Result<Shaders, GlesError> {
        use UniformType::*;
        let u = |name: &'static str, ty| UniformName::new(name, ty);
        let grid = renderer.compile_custom_pixel_shader(
            format!("{PRECISION}{GRID_FRAG}"),
            &[
                u("u_cam", _2f),
                u("u_px", _1f),
                u("u_spacing", _1f),
                u("u_fade", _1f),
                u("u_style", _1f),
                u("u_dpi", _1f),
                u("u_color", _4f),
            ],
        )?;
        let frame = renderer.compile_custom_pixel_shader(
            format!("{PRECISION}{FRAME_FRAG}"),
            &[
                u("u_frame", _4f),
                u("u_title", _1f),
                u("u_radius", _1f),
                u("u_border", _1f),
                u("u_shadow", _1f),
                u("u_c1", _4f),
                u("u_c2", _4f),
                u("u_title_col", _4f),
                u("u_shadow_col", _4f),
                u("u_btn", _4f),
                u("u_accent", _4f),
                u("u_max_col", _4f),
                u("u_mode", _1f),
            ],
        )?;
        let clip = renderer.compile_custom_texture_shader(
            CLIP_FRAG,
            &[u("u_uv_to_px", _4f), u("u_clip", _4f), u("u_radii", _2f)],
        )?;
        Ok(Shaders { grid, frame, clip })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ShaderValue {
    F1(f32),
    F2([f32; 2]),
    F4([f32; 4]),
}

impl ShaderValue {
    fn values(&self) -> &[f32] {
        match self {
            Self::F1(v) => std::slice::from_ref(v),
            Self::F2(v) => v,
            Self::F4(v) => v,
        }
    }

    fn uniform(self, name: &'static str) -> Uniform<'static> {
        match self {
            Self::F1(v) => Uniform::new(name, v),
            Self::F2(v) => Uniform::new(name, v),
            Self::F4(v) => Uniform::new(name, v),
        }
    }
}

/// A pixel-shader element positioned in exact physical pixels, so frames line
/// up with zoomed window content without logical-pixel rounding.
#[derive(Debug, Clone)]
pub struct ShaderElement {
    id: Id,
    commit: CommitCounter,
    program: Option<GlesPixelProgram>,
    rect: Rectangle<i32, Physical>,
    alpha: f32,
    params: Vec<f32>,
    uniforms: Vec<Uniform<'static>>,
}

impl Default for ShaderElement {
    fn default() -> Self {
        Self {
            id: Id::new(),
            commit: CommitCounter::default(),
            program: None,
            rect: Rectangle::default(),
            alpha: 1.0,
            params: Vec::new(),
            uniforms: Vec::new(),
        }
    }
}

impl ShaderElement {
    /// Updates geometry and uniforms; damage is only produced when something
    /// actually changed, so idle frames cost nothing.
    pub fn update(
        &mut self,
        program: &GlesPixelProgram,
        rect: Rectangle<i32, Physical>,
        alpha: f32,
        uniforms: &[(&'static str, ShaderValue)],
    ) -> &Self {
        let params: Vec<f32> = uniforms
            .iter()
            .flat_map(|(_, v)| v.values().iter().copied())
            .collect();
        if self.program.is_none()
            || self.rect != rect
            || self.alpha != alpha
            || self.params != params
        {
            self.program = Some(program.clone());
            self.rect = rect;
            self.alpha = alpha;
            self.params = params;
            self.uniforms = uniforms.iter().map(|(n, v)| v.uniform(n)).collect();
            self.commit.increment();
        }
        self
    }
}

impl Element for ShaderElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size(Size::from((
            self.rect.size.w as f64,
            self.rect.size.h as f64,
        )))
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.rect
    }

    fn alpha(&self) -> f32 {
        self.alpha
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R: PzRenderer> RenderElement<R> for ShaderElement {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        let Some(program) = &self.program else {
            return Ok(());
        };
        let size = Size::<i32, Buffer>::from((self.rect.size.w, self.rect.size.h));
        R::gles_frame(frame)
            .render_pixel_shader_to(
                program,
                src,
                dst,
                size,
                Some(damage),
                self.alpha,
                &self.uniforms,
            )
            .map_err(R::wrap_error)
    }

    fn underlying_storage(&self, _renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        None
    }
}

/// Rounded rectangle (output physical pixels) that client content is cut to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clip {
    pub rect: Rectangle<f64, Physical>,
    pub radius_top: f32,
    pub radius_bottom: f32,
}

impl Clip {
    /// Only surfaces that reach into a rounded corner need the shader.
    pub fn touches_corner(&self, g: Rectangle<i32, Physical>) -> bool {
        let g = g.to_f64();
        let c = self.rect;
        let near_side =
            |r: f64| g.loc.x < c.loc.x + r || g.loc.x + g.size.w > c.loc.x + c.size.w - r;
        let rt = self.radius_top as f64;
        let rb = self.radius_bottom as f64;
        (rt > 0.0 && g.loc.y < c.loc.y + rt && near_side(rt))
            || (rb > 0.0 && g.loc.y + g.size.h > c.loc.y + c.size.h - rb && near_side(rb))
    }
}

/// A (zoomed) client surface drawn through the clip shader.
pub struct ClippedSurface<R: smithay::backend::renderer::Renderer> {
    inner: RescaleRenderElement<WaylandSurfaceRenderElement<R>>,
    /// Size of the client texture; needed to map texture coordinates back
    /// to screen pixels. `None` for single-color buffers.
    tex_size: Option<Size<i32, Buffer>>,
    clip: Clip,
    program: GlesTexProgram,
}

impl<R: PzRenderer> ClippedSurface<R> {
    pub fn new(
        inner: RescaleRenderElement<WaylandSurfaceRenderElement<R>>,
        tex_size: Option<Size<i32, Buffer>>,
        clip: Clip,
        program: GlesTexProgram,
    ) -> Self {
        Self {
            inner,
            tex_size,
            clip,
            program,
        }
    }
}

impl<R: PzRenderer> Element for ClippedSurface<R> {
    fn id(&self) -> &Id {
        self.inner.id()
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
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
    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }
    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }
    // Rounded corners are transparent, so nothing here is opaque.
    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R: PzRenderer> RenderElement<R> for ClippedSurface<R> {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        let (Some(ts), Transform::Normal) = (self.tex_size, self.inner.transform()) else {
            return self.inner.draw(frame, src, dst, damage, opaque_regions);
        };
        // uv -> buffer pixel -> output pixel, inverting smithay's texture matrix.
        let kx = dst.size.w as f64 / src.size.w;
        let ky = dst.size.h as f64 / src.size.h;
        let uv_to_px = [
            (ts.w as f64 * kx) as f32,
            (ts.h as f64 * ky) as f32,
            (dst.loc.x as f64 - src.loc.x * kx) as f32,
            (dst.loc.y as f64 - src.loc.y * ky) as f32,
        ];
        let c = self.clip.rect;
        let uniforms = vec![
            Uniform::new("u_uv_to_px", uv_to_px),
            Uniform::new(
                "u_clip",
                [
                    c.loc.x as f32,
                    c.loc.y as f32,
                    c.size.w as f32,
                    c.size.h as f32,
                ],
            ),
            Uniform::new("u_radii", [self.clip.radius_top, self.clip.radius_bottom]),
        ];
        R::gles_frame(frame).override_default_tex_program(self.program.clone(), uniforms);
        let result = self.inner.draw(frame, src, dst, damage, &[]);
        R::gles_frame(frame).clear_tex_program_override();
        result
    }

    // Never hand the raw buffer to a scanout plane: that would skip the clip.
    fn underlying_storage(&self, _renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        None
    }
}
