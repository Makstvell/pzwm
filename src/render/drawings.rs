// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Drawings on the canvas (see draw.rs): one round-capped line element per
//! segment, above the windows so notes and arrows can point at them.

use std::collections::HashSet;

use smithay::{
    output::Output,
    utils::{Logical, Point, Rectangle, Size},
};

use super::{
    PzElement, PzRenderer, phys_rect,
    shaders::{ShaderValue, Shaders},
};
use crate::state::State;

pub(super) fn drawing_elements<R: PzRenderer>(
    state: &mut State,
    output: &Output,
    os: f64,
    sh: &Shaders,
    out: &mut Vec<PzElement<R>>,
) {
    let name = output.name();
    let ws = state.output_ws(output);
    if state.draw.hidden || state.draw.strokes[ws].is_empty() {
        state
            .render_cache
            .drawings
            .retain(|(o, _, _), _| *o != name);
        return;
    }
    let cam = state.camera(output);
    let screen = Rectangle::from_size(state.output_geometry(output).size.to_f64());
    let mut used = HashSet::new();
    // Newest strokes on top: the list is drawn front to back.
    for stroke in state.draw.strokes[ws].iter().rev() {
        let half = (stroke.width * cam.zoom / 2.0).max(0.5);
        for (k, (a, b)) in stroke.segments().into_iter().enumerate() {
            let a = cam.to_screen(Point::from((a[0], a[1])));
            let b = cam.to_screen(Point::from((b[0], b[1])));
            let lo = Point::<f64, Logical>::from((a.x.min(b.x) - half, a.y.min(b.y) - half));
            let size = Size::from((
                (a.x - b.x).abs() + 2.0 * half,
                (a.y - b.y).abs() + 2.0 * half,
            ));
            let rect = Rectangle::new(lo, size);
            if !rect.overlaps(screen) {
                continue;
            }
            let px = phys_rect(rect, os);
            // Endpoints relative to the element's pixel rectangle.
            let origin = px.loc.to_f64();
            let local = |p: Point<f64, Logical>| {
                let p = p.to_physical(os);
                [(p.x - origin.x) as f32, (p.y - origin.y) as f32]
            };
            let (la, lb) = (local(a), local(b));
            let uniforms = [
                ("u_seg", ShaderValue::F4([la[0], la[1], lb[0], lb[1]])),
                ("u_half", ShaderValue::F1((half * os) as f32)),
                ("u_color", ShaderValue::F4(stroke.color)),
            ];
            let key = (name.clone(), stroke.id, k);
            let elem = state.render_cache.drawings.entry(key.clone()).or_default();
            out.push(PzElement::Shader(
                elem.update(&sh.segment, px, 1.0, &uniforms).clone(),
            ));
            used.insert(key);
        }
    }
    state
        .render_cache
        .drawings
        .retain(|key, _| key.0 != name || used.contains(key));
}
