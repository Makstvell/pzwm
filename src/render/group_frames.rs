// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Group frames: a tinted rounded rectangle behind each group's windows,
//! filled stronger while the group is collapsed. Splits get a thin outline
//! a little outside their windows, brighter while one of them is focused.

use smithay::{
    output::Output,
    utils::{Logical, Rectangle},
};

use super::{
    PzElement, PzRenderer, phys_rect,
    shaders::{ShaderValue, Shaders},
};
use crate::{groups::group_color, state::State};

/// Corner radius of a frame, canvas units (it zooms with the windows).
const FRAME_RADIUS: f64 = 18.0;
/// Outline width, screen pixels (stays crisp at every zoom).
const FRAME_BORDER: f64 = 2.0;

pub(super) fn group_elements<R: PzRenderer>(
    state: &mut State,
    output: &Output,
    os: f64,
    sh: &Shaders,
    out: &mut Vec<PzElement<R>>,
) {
    let ws = state.output_ws(output);
    let frames = state.group_frames(ws);
    let name = output.name();
    state
        .render_cache
        .groups
        .retain(|(o, g), _| *o != name || frames.iter().any(|f| &f.name == g));
    if frames.is_empty() {
        return;
    }
    let cam = state.camera(output);
    let screen = Rectangle::from_size(state.output_geometry(output).size.to_f64());
    for frame in frames {
        let rect: Rectangle<f64, Logical> = Rectangle::new(
            cam.to_screen(frame.rect.loc),
            frame.rect.size.upscale(cam.zoom),
        );
        if !rect.overlaps(screen) {
            continue;
        }
        let [r, g, b] = group_color(&frame.name);
        let (fill, stroke) = if frame.collapsed {
            (0.32, 0.95)
        } else {
            (0.07, 0.7)
        };
        let uniforms = [
            ("u_fill", ShaderValue::F4([r, g, b, fill])),
            ("u_stroke", ShaderValue::F4([r, g, b, stroke])),
            (
                "u_radius",
                ShaderValue::F1((FRAME_RADIUS * cam.zoom * os) as f32),
            ),
            ("u_border", ShaderValue::F1((FRAME_BORDER * os) as f32)),
        ];
        let elem = state
            .render_cache
            .groups
            .entry((name.clone(), frame.name))
            .or_default();
        out.push(PzElement::Shader(
            elem.update(&sh.rect, phys_rect(rect, os), 1.0, &uniforms)
                .clone(),
        ));
    }
}

/// Space between a split's windows and its outline, screen pixels.
const SPLIT_PAD: f64 = 7.0;
const SPLIT_BORDER: f64 = 1.5;

pub(super) fn split_elements<R: PzRenderer>(
    state: &mut State,
    output: &Output,
    os: f64,
    sh: &Shaders,
    out: &mut Vec<PzElement<R>>,
) {
    let ws = state.output_ws(output);
    let outlines = state.split_outlines(ws);
    let name = output.name();
    state
        .render_cache
        .splits
        .retain(|(o, id), _| *o != name || outlines.iter().any(|(s, _, _)| s == id));
    let cam = state.camera(output);
    let screen = Rectangle::from_size(state.output_geometry(output).size.to_f64());
    place_elements(state, output, os, sh, out);
    let [r, g, b, _] = state.config.appearance.active_border_start;
    let radius = state.config.appearance.corner_radius as f64 * cam.zoom + SPLIT_PAD;
    for (id, area, focused) in outlines {
        let inner: Rectangle<f64, Logical> =
            Rectangle::new(cam.to_screen(area.loc), area.size.upscale(cam.zoom));
        let rect = Rectangle::new(
            (inner.loc.x - SPLIT_PAD, inner.loc.y - SPLIT_PAD).into(),
            (
                inner.size.w + 2.0 * SPLIT_PAD,
                inner.size.h + 2.0 * SPLIT_PAD,
            )
                .into(),
        );
        if !rect.overlaps(screen) {
            continue;
        }
        let stroke = if focused { 0.85 } else { 0.35 };
        let uniforms = [
            ("u_fill", ShaderValue::F4([r, g, b, 0.0])),
            ("u_stroke", ShaderValue::F4([r, g, b, stroke])),
            ("u_radius", ShaderValue::F1((radius * os) as f32)),
            ("u_border", ShaderValue::F1((SPLIT_BORDER * os) as f32)),
        ];
        let elem = state
            .render_cache
            .splits
            .entry((name.clone(), id))
            .or_default();
        out.push(PzElement::Shader(
            elem.update(&sh.rect, phys_rect(rect, os), 1.0, &uniforms)
                .clone(),
        ));
    }
}

/// Length and thickness of the "+" in an empty place, screen pixels.
const PLUS_LEN: f64 = 44.0;
const PLUS_THICK: f64 = 6.0;

/// Empty places of a split being filled: a tinted outlined area with a
/// "+", the next one to be filled brighter.
fn place_elements<R: PzRenderer>(
    state: &mut State,
    output: &Output,
    os: f64,
    sh: &Shaders,
    out: &mut Vec<PzElement<R>>,
) {
    let ws = state.output_ws(output);
    let places = state.split_empty_places(ws);
    let name = output.name();
    state.render_cache.places.retain(|(o, s, i, _), _| {
        *o != name || places.iter().any(|(ps, pi, _, _)| ps == s && pi == i)
    });
    let cam = state.camera(output);
    let [r, g, b, _] = state.config.appearance.active_border_end;
    let radius = state.config.appearance.corner_radius as f64 * cam.zoom;
    for (split, i, area, next) in places {
        let rect: Rectangle<f64, Logical> =
            Rectangle::new(cam.to_screen(area.loc), area.size.upscale(cam.zoom));
        let (fill, stroke) = if next { (0.20, 1.0) } else { (0.09, 0.6) };
        let c = rect.loc + rect.size.downscale(2.0).to_point();
        let bars = [
            Rectangle::new(
                (c.x - PLUS_LEN / 2.0, c.y - PLUS_THICK / 2.0).into(),
                (PLUS_LEN, PLUS_THICK).into(),
            ),
            Rectangle::new(
                (c.x - PLUS_THICK / 2.0, c.y - PLUS_LEN / 2.0).into(),
                (PLUS_THICK, PLUS_LEN).into(),
            ),
        ];
        // Front to back: the "+", then the place.
        for (part, bar) in bars.into_iter().enumerate() {
            let uniforms = [
                ("u_fill", ShaderValue::F4([r, g, b, stroke])),
                ("u_stroke", ShaderValue::F4([r, g, b, stroke])),
                ("u_radius", ShaderValue::F1((PLUS_THICK / 2.0 * os) as f32)),
                ("u_border", ShaderValue::F1(0.0)),
            ];
            let elem = state
                .render_cache
                .places
                .entry((name.clone(), split, i, 1 + part as u8))
                .or_default();
            out.push(PzElement::Shader(
                elem.update(&sh.rect, phys_rect(bar, os), 1.0, &uniforms)
                    .clone(),
            ));
        }
        let uniforms = [
            ("u_fill", ShaderValue::F4([r, g, b, fill])),
            ("u_stroke", ShaderValue::F4([r, g, b, stroke])),
            ("u_radius", ShaderValue::F1((radius * os) as f32)),
            ("u_border", ShaderValue::F1((2.0 * os) as f32)),
        ];
        let elem = state
            .render_cache
            .places
            .entry((name.clone(), split, i, 0))
            .or_default();
        out.push(PzElement::Shader(
            elem.update(&sh.rect, phys_rect(rect, os), 1.0, &uniforms)
                .clone(),
        ));
    }
}
