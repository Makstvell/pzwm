//! Group frames: a tinted rounded rectangle behind each group's windows,
//! filled stronger while the group is collapsed.

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
