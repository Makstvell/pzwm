//! Builds the element list for one output, front to back:
//! cursor → lock screen → overlay layer → fullscreen → bar (top layer) →
//! snap preview → windows with their frames → canvas grid → bottom and
//! background layers (the wallpaper).

pub mod shaders;

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, RendererSuper, Texture,
        element::{
            AsRenderElements, Kind,
            memory::MemoryRenderBufferRenderElement,
            render_elements,
            surface::{
                WaylandSurfaceRenderElement, WaylandSurfaceTexture,
                render_elements_from_surface_tree,
            },
            utils::RescaleRenderElement,
        },
        gles::{GlesError, GlesFrame, GlesRenderer},
    },
    desktop::{
        PopupManager, layer_map_for_output, space::SpaceElement, utils::send_frames_surface_tree,
    },
    input::pointer::{CursorImageStatus, CursorImageSurfaceData},
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale, Size},
    wayland::{compositor::with_states, shell::wlr_layer::Layer},
};

use crate::{
    canvas::grid_level,
    shell::{Placement, View},
    state::State,
};
use shaders::{Clip, ClippedSurface, ShaderElement, ShaderValue, Shaders};

/// Renderers pzwm can draw with: plain GLES (nested) and the multi-GPU
/// renderer (on a TTY). Both expose the underlying GLES frame so the custom
/// shaders work everywhere.
pub trait PzRenderer:
    RendererSuper<TextureId: Clone + Send + 'static> + Renderer + ImportAll + ImportMem
{
    fn gles(&mut self) -> &mut GlesRenderer;
    fn gles_frame<'a, 'frame, 'buffer>(
        frame: &'a mut Self::Frame<'frame, 'buffer>,
    ) -> &'a mut GlesFrame<'frame, 'buffer>
    where
        'buffer: 'frame,
        Self: 'frame;
    fn wrap_error(err: GlesError) -> Self::Error;
}

impl PzRenderer for GlesRenderer {
    fn gles(&mut self) -> &mut GlesRenderer {
        self
    }
    fn gles_frame<'a, 'frame, 'buffer>(
        frame: &'a mut GlesFrame<'frame, 'buffer>,
    ) -> &'a mut GlesFrame<'frame, 'buffer>
    where
        'buffer: 'frame,
    {
        frame
    }
    fn wrap_error(err: GlesError) -> GlesError {
        err
    }
}

render_elements! {
    pub PzElement<R> where R: PzRenderer;
    Surface = WaylandSurfaceRenderElement<R>,
    Zoomed = RescaleRenderElement<WaylandSurfaceRenderElement<R>>,
    Clipped = ClippedSurface<R>,
    Shader = ShaderElement,
    Memory = MemoryRenderBufferRenderElement<R>,
}

/// Shader elements keep their identity between frames so damage tracking
/// only repaints what changed.
#[derive(Default)]
pub struct RenderCache {
    frames: HashMap<u64, ShaderElement>,
    grids: HashMap<String, ShaderElement>,
    preview: ShaderElement,
}

impl RenderCache {
    pub fn forget(&mut self, window_id: u64) {
        self.frames.remove(&window_id);
    }
}

fn phys_rect(r: Rectangle<f64, Logical>, scale: f64) -> Rectangle<i32, Physical> {
    let loc = r.loc.to_physical(scale).to_i32_round::<i32>();
    let br = (r.loc + r.size.to_point())
        .to_physical(scale)
        .to_i32_round::<i32>();
    Rectangle::new(loc, Size::from((br.x - loc.x, br.y - loc.y)))
}

/// Everything to draw on `output`, front to back, and the clear color.
pub fn output_elements<R: PzRenderer>(
    state: &mut State,
    renderer: &mut R,
    output: &Output,
) -> (Vec<PzElement<R>>, [f32; 4]) {
    let os = output.current_scale().fractional_scale();
    let shaders = Shaders::get(renderer.gles());
    let now = Instant::now();
    let mut out: Vec<PzElement<R>> = Vec::new();

    cursor_elements(state, renderer, output, os, &mut out);

    if state.lock.is_locked() {
        if let Some(lock) = state.lock_surfaces.get(&output.name()) {
            out.extend(
                render_elements_from_surface_tree(
                    renderer,
                    lock.wl_surface(),
                    (0, 0),
                    os,
                    1.0,
                    Kind::Unspecified,
                )
                .into_iter()
                .map(PzElement::Surface),
            );
        }
        return (out, [0.0, 0.0, 0.0, 1.0]);
    }

    layer_elements(renderer, output, Layer::Overlay, os, &mut out);

    let stacking = state.stacking(output);
    let fullscreen = stacking
        .iter()
        .copied()
        .find(|&i| state.windows[i].placement == Placement::Fullscreen);
    if let Some(i) = fullscreen {
        window_elements(
            state,
            renderer,
            output,
            i,
            os,
            shaders.as_ref(),
            now,
            &mut out,
        );
    } else {
        layer_elements(renderer, output, Layer::Top, os, &mut out);
    }

    // The edge-tiling preview sits right under the window being dragged.
    let mut preview = match (state.snap_preview.clone(), shaders.as_ref()) {
        (Some((o, rect)), Some(sh)) if &o == output => Some(preview_element(state, rect, os, sh)),
        _ => None,
    };
    for i in stacking.into_iter().filter(|&i| Some(i) != fullscreen) {
        window_elements(
            state,
            renderer,
            output,
            i,
            os,
            shaders.as_ref(),
            now,
            &mut out,
        );
        if let Some(p) = preview.take() {
            out.push(PzElement::Shader(p));
        }
    }
    if let Some(p) = preview {
        out.push(PzElement::Shader(p));
    }

    if let Some(sh) = shaders.as_ref() {
        if let Some(grid) = grid_element(state, output, os, sh) {
            out.push(PzElement::Shader(grid));
        }
    }

    layer_elements(renderer, output, Layer::Bottom, os, &mut out);
    layer_elements(renderer, output, Layer::Background, os, &mut out);
    (out, state.config.appearance.background)
}

fn layer_elements<R: PzRenderer>(
    renderer: &mut R,
    output: &Output,
    layer: Layer,
    os: f64,
    out: &mut Vec<PzElement<R>>,
) {
    let map = layer_map_for_output(output);
    for surface in map.layers_on(layer).rev() {
        let Some(geo) = map.layer_geometry(surface) else {
            continue;
        };
        let loc = geo.loc.to_physical_precise_round(os);
        out.extend(
            AsRenderElements::<R>::render_elements::<WaylandSurfaceRenderElement<R>>(
                surface,
                renderer,
                loc,
                Scale::from(os),
                1.0,
            )
            .into_iter()
            .map(PzElement::Surface),
        );
    }
}

fn cursor_elements<R: PzRenderer>(
    state: &mut State,
    renderer: &mut R,
    output: &Output,
    os: f64,
    out: &mut Vec<PzElement<R>>,
) {
    if !state.render_cursor || state.output_at(state.cursor).as_ref() != Some(output) {
        return;
    }
    let local = state.cursor_local(output);
    let icon = match (&state.cursor_override, &state.cursor_status) {
        (Some(icon), _) => *icon,
        (None, CursorImageStatus::Hidden) => return,
        (None, CursorImageStatus::Named(icon)) => *icon,
        (None, CursorImageStatus::Surface(surface)) => {
            let hotspot = with_states(surface, |states| {
                states
                    .data_map
                    .get::<CursorImageSurfaceData>()
                    .and_then(|d| d.lock().ok().map(|d| d.hotspot))
                    .unwrap_or_default()
            });
            let pos = (local - hotspot.to_f64()).to_physical(os).to_i32_round();
            out.extend(
                render_elements_from_surface_tree(renderer, surface, pos, os, 1.0, Kind::Cursor)
                    .into_iter()
                    .map(PzElement::Surface),
            );
            return;
        }
    };
    let Some(image) = state.cursor_theme.get(icon, os.ceil() as i32) else {
        return;
    };
    let pos = (local - image.hotspot.to_f64()).to_physical(os);
    if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        pos,
        &image.buffer,
        None,
        None,
        None,
        Kind::Cursor,
    ) {
        out.push(PzElement::Memory(elem));
    }
}

#[allow(clippy::too_many_arguments)]
fn window_elements<R: PzRenderer>(
    state: &mut State,
    renderer: &mut R,
    output: &Output,
    idx: usize,
    os: f64,
    shaders: Option<&Shaders>,
    now: Instant,
    out: &mut Vec<PzElement<R>>,
) {
    let m = &state.windows[idx];
    let Some(base) = state.view(m, output) else {
        return;
    };
    let Some(surface) = m.wl_surface() else {
        return;
    };

    // Open/maximize animation: scale around the frame center, fade in.
    let (grow, alpha) = state.open_effect(m, now);
    let frame0 = state.frame_rect(m, &base);
    let center = frame0.loc + frame0.size.downscale(2.0).to_point();
    let view = View {
        origin: center + (base.origin - center).upscale(grow),
        scale: base.scale * grow,
    };
    let frame = state.frame_rect(m, &view);
    let appearance = &state.config.appearance;
    let title = state.title_height(m);
    let geo = m.window.geometry();
    let size = geo.size.to_f64();
    let fullscreen = m.placement == Placement::Fullscreen;
    let zoom_k = view.scale.min(1.0);
    let radius = if fullscreen {
        0.0
    } else {
        appearance.corner_radius as f64 * zoom_k
    };
    let border = if fullscreen {
        0.0
    } else {
        appearance.border_width as f64
    };
    let inner_radius = (radius - border).max(0.0);

    // Surfaces are laid out at output scale from the surface origin, then
    // the whole tree is zoomed around that origin.
    let zoom = Scale::from(view.scale);
    let surface_origin = view.origin - geo.loc.to_f64().upscale(view.scale);
    let surface_phys = surface_origin.to_physical(os).to_i32_round();

    for (popup, offset) in PopupManager::popups_for_surface(&surface) {
        let o = (geo.loc + offset - popup.geometry().loc)
            .to_f64()
            .upscale(os);
        let loc = Point::<i32, Physical>::from((
            surface_phys.x + o.x.round() as i32,
            surface_phys.y + o.y.round() as i32,
        ));
        let elems: Vec<WaylandSurfaceRenderElement<R>> = render_elements_from_surface_tree(
            renderer,
            popup.wl_surface(),
            loc,
            os,
            alpha,
            Kind::Unspecified,
        );
        out.extend(
            elems.into_iter().map(|e| {
                PzElement::Zoomed(RescaleRenderElement::from_element(e, surface_phys, zoom))
            }),
        );
    }

    let clip = Clip {
        rect: Rectangle::new(view.origin, size.upscale(view.scale)).to_physical(os),
        radius_top: if title > 0.0 {
            0.0
        } else {
            (inner_radius * os) as f32
        },
        radius_bottom: (inner_radius * os) as f32,
    };
    let elems: Vec<WaylandSurfaceRenderElement<R>> = render_elements_from_surface_tree(
        renderer,
        &surface,
        surface_phys,
        os,
        alpha,
        Kind::Unspecified,
    );
    for elem in elems {
        use smithay::backend::renderer::element::Element;
        let tex_size = match elem.texture() {
            WaylandSurfaceTexture::Texture(tex) => Some(tex.size()),
            WaylandSurfaceTexture::SolidColor(_) => None,
        };
        let zoomed = RescaleRenderElement::from_element(elem, surface_phys, zoom);
        let clipped =
            clip.radius_bottom > 0.5 && clip.touches_corner(zoomed.geometry(Scale::from(os)));
        match (clipped, shaders) {
            (true, Some(sh)) => out.push(PzElement::Clipped(ClippedSurface::new(
                zoomed,
                tex_size,
                clip,
                sh.clip.clone(),
            ))),
            _ => out.push(PzElement::Zoomed(zoomed)),
        }
    }

    if fullscreen {
        return;
    }
    let Some(sh) = shaders else { return };
    let focused = state.focused.as_ref() == Some(&m.window);
    let pinned = m.placement == Placement::Pinned;
    let maximized = m.placement == Placement::Maximized;
    let (c1, c2) = if pinned {
        (appearance.pinned_accent, appearance.pinned_accent)
    } else if focused {
        (appearance.active_border_start, appearance.active_border_end)
    } else {
        (appearance.inactive_border, appearance.inactive_border)
    };
    let title_col = if focused {
        appearance.titlebar_active_color
    } else {
        appearance.titlebar_color
    };
    let shadow = appearance.shadow_size as f64 * zoom_k.sqrt() * if focused { 1.0 } else { 0.7 };
    let margin = shadow + border + 2.0;
    let outer = Rectangle::new(
        frame.loc - Point::from((margin, margin)),
        frame.size + Size::from((2.0 * margin, 2.0 * margin)),
    );
    let rect = phys_rect(outer, os);
    let frame_local = phys_rect(frame, os);
    let hover = state
        .hover
        .filter(|h| h.id == m.id)
        .map(|h| h.button.index())
        .unwrap_or(0.0);
    let buttons_visible = title > 0.0 && title * view.scale >= 14.0;
    let uniforms = [
        (
            "u_frame",
            ShaderValue::F4([
                (frame_local.loc.x - rect.loc.x) as f32,
                (frame_local.loc.y - rect.loc.y) as f32,
                frame_local.size.w as f32,
                frame_local.size.h as f32,
            ]),
        ),
        ("u_title", ShaderValue::F1((title * view.scale * os) as f32)),
        ("u_radius", ShaderValue::F1((radius * os) as f32)),
        ("u_border", ShaderValue::F1((border * os) as f32)),
        ("u_shadow", ShaderValue::F1((shadow * os) as f32)),
        ("u_c1", ShaderValue::F4(c1)),
        ("u_c2", ShaderValue::F4(c2)),
        ("u_title_col", ShaderValue::F4(title_col)),
        ("u_shadow_col", ShaderValue::F4(appearance.shadow_color)),
        (
            "u_btn",
            ShaderValue::F4([
                if buttons_visible { 1.0 } else { 0.0 },
                hover,
                if pinned { 1.0 } else { 0.0 },
                if maximized { 1.0 } else { 0.0 },
            ]),
        ),
        ("u_accent", ShaderValue::F4(appearance.pinned_accent)),
        (
            "u_max_col",
            ShaderValue::F4(if maximized {
                appearance.active_border_start
            } else {
                appearance.active_border_end
            }),
        ),
        ("u_mode", ShaderValue::F1(0.0)),
    ];
    let id = m.id;
    let elem = state.render_cache.frames.entry(id).or_default();
    out.push(PzElement::Shader(
        elem.update(&sh.frame, rect, alpha, &uniforms).clone(),
    ));
}

fn grid_element(
    state: &mut State,
    output: &Output,
    os: f64,
    sh: &Shaders,
) -> Option<ShaderElement> {
    let style = match state.config.canvas.grid.as_str() {
        "dots" => 1.0,
        "lines" => 2.0,
        _ => return None,
    };
    let cam = state.camera(output);
    let size = state.output_geometry(output).size.to_f64();
    let rect = phys_rect(Rectangle::from_size(size), os);
    let (spacing, fade) = grid_level(state.config.canvas.grid_spacing, cam.zoom);
    let uniforms = [
        (
            "u_cam",
            ShaderValue::F2([cam.pos.x as f32, cam.pos.y as f32]),
        ),
        ("u_px", ShaderValue::F1((cam.zoom * os) as f32)),
        ("u_spacing", ShaderValue::F1(spacing as f32)),
        ("u_fade", ShaderValue::F1(fade as f32)),
        ("u_style", ShaderValue::F1(style)),
        ("u_dpi", ShaderValue::F1(os as f32)),
        (
            "u_color",
            ShaderValue::F4(state.config.appearance.grid_color),
        ),
    ];
    let elem = state.render_cache.grids.entry(output.name()).or_default();
    Some(elem.update(&sh.grid, rect, 1.0, &uniforms).clone())
}

fn preview_element(
    state: &mut State,
    rect: Rectangle<f64, Logical>,
    os: f64,
    sh: &Shaders,
) -> ShaderElement {
    let a = &state.config.appearance;
    let r = phys_rect(rect, os);
    let uniforms = [
        (
            "u_frame",
            ShaderValue::F4([0.0, 0.0, r.size.w as f32, r.size.h as f32]),
        ),
        ("u_title", ShaderValue::F1(0.0)),
        (
            "u_radius",
            ShaderValue::F1((a.corner_radius as f64 * os) as f32),
        ),
        (
            "u_border",
            ShaderValue::F1((a.border_width as f64 * os) as f32),
        ),
        ("u_shadow", ShaderValue::F1(0.0)),
        ("u_c1", ShaderValue::F4(a.active_border_start)),
        ("u_c2", ShaderValue::F4(a.active_border_end)),
        ("u_title_col", ShaderValue::F4(a.titlebar_color)),
        ("u_shadow_col", ShaderValue::F4(a.shadow_color)),
        ("u_btn", ShaderValue::F4([0.0; 4])),
        ("u_accent", ShaderValue::F4(a.pinned_accent)),
        ("u_max_col", ShaderValue::F4(a.active_border_end)),
        ("u_mode", ShaderValue::F1(1.0)),
    ];
    state
        .render_cache
        .preview
        .update(&sh.frame, r, 1.0, &uniforms)
        .clone()
}

/// After a frame: tell surfaces which outputs show them and send frame
/// callbacks so clients draw their next frame.
pub fn post_render(state: &mut State, output: &Output) {
    let time = state.clock.now();
    let first_output = state.outputs.first() == Some(output);
    let output_rect = Rectangle::from_size(state.output_geometry(output).size).to_f64();

    for m in &state.windows {
        let visible = state.view(m, output).and_then(|view| {
            let frame = state.frame_rect(m, &view);
            frame.intersection(output_rect)
        });
        match visible {
            Some(overlap) => {
                m.window.output_enter(output, overlap.to_i32_round());
                m.window
                    .send_frame(output, time, Some(Duration::ZERO), |_, _| {
                        Some(output.clone())
                    });
            }
            None => {
                m.window.output_leave(output);
                if first_output {
                    // Hidden windows still get a slow tick so they never freeze.
                    m.window
                        .send_frame(output, time, Some(Duration::from_millis(995)), |_, _| None);
                }
            }
        }
    }

    let map = layer_map_for_output(output);
    for layer in map.layers() {
        layer.send_frame(output, time, Some(Duration::ZERO), |_, _| {
            Some(output.clone())
        });
    }
    drop(map);

    if let Some(lock) = state.lock_surfaces.get(&output.name()) {
        send_frames_surface_tree(
            lock.wl_surface(),
            output,
            time,
            Some(Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }
    if let CursorImageStatus::Surface(surface) = &state.cursor_status {
        send_frames_surface_tree(surface, output, time, Some(Duration::ZERO), |_, _| {
            Some(output.clone())
        });
    }
}
