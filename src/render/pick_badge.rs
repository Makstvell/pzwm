// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! While new windows wait for their place ("pick" mode), a card next to
//! the cursor shows which one is being placed: a live miniature of the
//! window with its app icon in the corner, and one dot per waiting window,
//! the current one lit. pzwm draws no text; the notification names the app.

use smithay::{
    backend::renderer::{
        Texture,
        element::{
            Element, Id, Kind,
            memory::MemoryRenderBufferRenderElement,
            surface::{
                WaylandSurfaceRenderElement, WaylandSurfaceTexture,
                render_elements_from_surface_tree,
            },
            utils::RescaleRenderElement,
        },
    },
    output::Output,
    utils::{Logical, Point, Rectangle, Scale, Size},
};

use super::{
    PzElement, PzRenderer, phys_rect,
    shaders::{Clip, ClippedSurface, ShaderValue, Shaders},
};
use crate::state::State;

/// Space between the cursor and the card, logical pixels.
const OFFSET: f64 = 24.0;
const WIDTH: f64 = 220.0;
const PAD: f64 = 10.0;
const PREVIEW_MIN_H: f64 = 60.0;
const PREVIEW_MAX_H: f64 = 140.0;
const ICON: f64 = 36.0;
const DOT: f64 = 8.0;
const DOT_GAP: f64 = 6.0;
/// More waiting windows than this show as this many dots.
const MAX_DOTS: usize = 12;
const RADIUS: f64 = 12.0;

/// Where the parts of the card go (output-local logical pixels).
#[derive(Debug, PartialEq)]
pub struct BadgeLayout {
    pub card: Rectangle<f64, Logical>,
    pub preview: Rectangle<f64, Logical>,
    pub icon: Rectangle<f64, Logical>,
    pub dots: Vec<Rectangle<f64, Logical>>,
}

/// The card for `count` waiting windows, below right of `cursor`, or on the
/// other side where `screen` ends. `aspect` is the window's height / width.
pub fn badge_layout(
    cursor: Point<f64, Logical>,
    screen: Size<f64, Logical>,
    aspect: Option<f64>,
    count: usize,
) -> BadgeLayout {
    let preview_w = WIDTH - 2.0 * PAD;
    let preview_h = aspect
        .filter(|a| a.is_finite() && *a > 0.0)
        .map_or(preview_w * 9.0 / 16.0, |a| preview_w * a)
        .clamp(PREVIEW_MIN_H, PREVIEW_MAX_H);
    let dots = count.min(MAX_DOTS);
    let dots_h = if dots > 1 { PAD + DOT } else { 0.0 };
    let height = PAD + preview_h + dots_h + PAD;
    let mut x = cursor.x + OFFSET;
    let mut y = cursor.y + OFFSET;
    if x + WIDTH > screen.w {
        x = cursor.x - OFFSET - WIDTH;
    }
    if y + height > screen.h {
        y = cursor.y - OFFSET - height;
    }
    let (x, y) = (x.max(0.0), y.max(0.0));
    let card = Rectangle::new((x, y).into(), (WIDTH, height).into());
    let preview = Rectangle::new((x + PAD, y + PAD).into(), (preview_w, preview_h).into());
    let icon = Rectangle::new(
        (preview.loc.x + 8.0, preview.loc.y + preview_h - ICON - 8.0).into(),
        (ICON, ICON).into(),
    );
    let row_w = dots as f64 * DOT + dots.saturating_sub(1) as f64 * DOT_GAP;
    let row_x = x + (WIDTH - row_w) / 2.0;
    let row_y = preview.loc.y + preview_h + PAD;
    let dots = if dots > 1 {
        (0..dots)
            .map(|i| {
                Rectangle::new(
                    (row_x + i as f64 * (DOT + DOT_GAP), row_y).into(),
                    (DOT, DOT).into(),
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    BadgeLayout {
        card,
        preview,
        icon,
        dots,
    }
}

/// The card, when a window waits for its place and the cursor is on `output`.
pub(super) fn badge_elements<R: PzRenderer>(
    state: &mut State,
    renderer: &mut R,
    output: &Output,
    os: f64,
    sh: &Shaders,
    out: &mut Vec<PzElement<R>>,
) {
    let current = state
        .area
        .window
        .filter(|_| state.area.selecting && !state.lock.is_locked())
        .and_then(|id| state.index_of_id(id));
    let on_output = state.output_at(state.cursor).as_ref() == Some(output);
    let Some(idx) = current.filter(|_| on_output) else {
        state.render_cache.badge.clear();
        state.render_cache.badge_thumbs.clear();
        return;
    };
    let count = 1 + state
        .area
        .waiting
        .iter()
        .filter(|&&id| {
            state
                .index_of_id(id)
                .is_some_and(|i| state.windows[i].awaiting_pick)
        })
        .count();
    let geo = state.windows[idx].window.geometry();
    let aspect = (geo.size.w > 0).then(|| geo.size.h as f64 / geo.size.w as f64);
    let screen = state.output_geometry(output).size.to_f64();
    let layout = badge_layout(state.cursor_local(output), screen, aspect, count);
    let a = state.config.appearance.clone();

    // Front to back: icon, dots, miniature, card.
    let app_id = state.app_id(&state.windows[idx]).unwrap_or_default();
    if let Some((icon, icon_size)) = state.icons.get(&app_id) {
        let size = Size::from((ICON as i32, ICON as i32));
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            layout.icon.loc.to_physical(os),
            &icon,
            None,
            Some(Rectangle::from_size(icon_size.to_f64())),
            Some(size),
            Kind::Unspecified,
        ) {
            out.push(PzElement::Memory(elem));
        }
    }
    let [r, g, b, _] = a.active_border_end;
    for (i, dot) in layout.dots.iter().enumerate() {
        let fill = if i == 0 {
            [r, g, b, 1.0]
        } else {
            [1.0, 1.0, 1.0, 0.30]
        };
        let uniforms = [
            ("u_fill", ShaderValue::F4(fill)),
            ("u_stroke", ShaderValue::F4(fill)),
            ("u_radius", ShaderValue::F1((DOT / 2.0 * os) as f32)),
            ("u_border", ShaderValue::F1(0.0)),
        ];
        let elem = state.render_cache.badge.entry(i + 1).or_default();
        out.push(PzElement::Shader(
            elem.update(&sh.rect, phys_rect(*dot, os), 1.0, &uniforms)
                .clone(),
        ));
    }
    let dots = layout.dots.len();
    state.render_cache.badge.retain(|&k, _| k == 0 || k <= dots);

    // The window itself, scaled to the preview's width and cut to it.
    let mut used = Vec::new();
    if let Some(surface) = state.windows[idx].wl_surface() {
        let scale = if geo.size.w > 0 {
            layout.preview.size.w / geo.size.w as f64
        } else {
            1.0
        };
        let origin = (layout.preview.loc - geo.loc.to_f64().upscale(scale))
            .to_physical(os)
            .to_i32_round();
        let clip = Clip {
            rect: layout.preview.to_physical(os),
            radius_top: (6.0 * os) as f32,
            radius_bottom: (6.0 * os) as f32,
        };
        let elems: Vec<WaylandSurfaceRenderElement<R>> = render_elements_from_surface_tree(
            renderer,
            &surface,
            origin,
            os,
            1.0,
            Kind::Unspecified,
        );
        for elem in elems {
            let tex_size = match elem.texture() {
                WaylandSurfaceTexture::Texture(tex) => Some(tex.size()),
                WaylandSurfaceTexture::SolidColor(_) => None,
            };
            let key = elem.id().clone();
            let zoomed = RescaleRenderElement::from_element(elem, origin, Scale::from(scale));
            let id = state
                .render_cache
                .badge_thumbs
                .entry(key.clone())
                .or_insert_with(Id::new)
                .clone();
            used.push(key);
            out.push(PzElement::Clipped(
                ClippedSurface::new(zoomed, tex_size, clip, sh.clip.clone()).with_id(id),
            ));
        }
    }
    state
        .render_cache
        .badge_thumbs
        .retain(|key, _| used.contains(key));

    let [cr, cg, cb, _] = a.titlebar_active_color;
    let uniforms = [
        ("u_fill", ShaderValue::F4([cr, cg, cb, 0.96])),
        ("u_stroke", ShaderValue::F4(a.active_border_start)),
        ("u_radius", ShaderValue::F1((RADIUS * os) as f32)),
        ("u_border", ShaderValue::F1((1.5 * os) as f32)),
    ];
    let elem = state.render_cache.badge.entry(0).or_default();
    out.push(PzElement::Shader(
        elem.update(&sh.rect, phys_rect(layout.card, os), 1.0, &uniforms)
            .clone(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f64, f64) = (1707.0, 960.0);

    #[test]
    fn the_card_sits_below_right_of_the_cursor() {
        let l = badge_layout((100.0, 100.0).into(), SCREEN.into(), Some(0.5), 3);
        assert_eq!(l.card.loc, Point::from((124.0, 124.0)));
        assert_eq!(l.preview.size, Size::from((200.0, 100.0)));
        assert_eq!(l.dots.len(), 3);
        // The dots are centered under the preview.
        let row_mid = (l.dots[0].loc.x + l.dots[2].loc.x + DOT) / 2.0;
        assert!((row_mid - (l.card.loc.x + WIDTH / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn near_the_edges_it_flips_to_the_other_side() {
        let l = badge_layout((1650.0, 920.0).into(), SCREEN.into(), Some(0.5), 1);
        assert!(l.card.loc.x + l.card.size.w <= 1650.0);
        assert!(l.card.loc.y + l.card.size.h <= 920.0);
    }

    #[test]
    fn one_window_shows_no_dots_and_many_are_capped() {
        let one = badge_layout((0.0, 0.0).into(), SCREEN.into(), None, 1);
        assert!(one.dots.is_empty());
        let many = badge_layout((0.0, 0.0).into(), SCREEN.into(), None, 40);
        assert_eq!(many.dots.len(), MAX_DOTS);
    }

    #[test]
    fn tall_windows_keep_a_bounded_preview() {
        let l = badge_layout((0.0, 0.0).into(), SCREEN.into(), Some(5.0), 1);
        assert_eq!(l.preview.size.h, PREVIEW_MAX_H);
    }
}
