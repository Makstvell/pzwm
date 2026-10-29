//! Semantic zoom: below `[canvas] semantic_zoom` the unreadable miniature
//! of each window fades into a card in its app's color with the app icon
//! in the middle, so a zoomed-out canvas reads like a map of apps.

use smithay::{
    backend::renderer::element::{Kind, memory::MemoryRenderBufferRenderElement},
    output::Output,
    utils::{Logical, Point, Rectangle, Size},
};

use super::{
    PzElement, PzRenderer, phys_rect,
    shaders::{ShaderValue, Shaders},
};
use crate::{groups::group_color, shell::Placement, state::State};

/// The card is fully there this far (share of the threshold) below it.
const FADE_SPAN: f64 = 0.35;
/// Icon side as a share of the card's smaller side, and at most (px).
const ICON_SHARE: f64 = 0.45;
const ICON_MAX_PX: f64 = 192.0;

/// 0 above the threshold, rising to 1 a little below it.
pub fn card_opacity(threshold: f64, zoom: f64) -> f64 {
    if threshold <= 0.0 {
        return 0.0;
    }
    ((threshold - zoom) / (threshold * FADE_SPAN)).clamp(0.0, 1.0)
}

/// Card and icon of window `idx`, pushed in front of the window itself.
pub(super) fn card_elements<R: PzRenderer>(
    state: &mut State,
    renderer: &mut R,
    output: &Output,
    idx: usize,
    os: f64,
    sh: &Shaders,
    out: &mut Vec<PzElement<R>>,
) {
    let m = &state.windows[idx];
    let id = m.id;
    let cam = state.camera(output);
    let opacity = card_opacity(state.config.canvas.semantic_zoom, cam.zoom);
    if opacity <= 0.0 || m.placement != Placement::Canvas {
        state.render_cache.cards.remove(&id);
        return;
    }
    let frame = state.canvas_frame(m);
    let rect: Rectangle<f64, Logical> =
        Rectangle::new(cam.to_screen(frame.loc), frame.size.upscale(cam.zoom));
    let app_id = state.app_id(m).unwrap_or_default();

    let side = (rect.size.w.min(rect.size.h) * ICON_SHARE).min(ICON_MAX_PX);
    if side >= 8.0 {
        if let Some((icon, icon_size)) = state.icons.get(&app_id) {
            let center = rect.loc + rect.size.downscale(2.0).to_point();
            let loc = center - Point::from((side / 2.0, side / 2.0));
            let size = Size::from((side.round() as i32, side.round() as i32));
            if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                loc.to_physical(os),
                &icon,
                Some(opacity as f32),
                // The whole image, scaled to `size` (without it smithay
                // would cut a `size` piece out of the top-left corner).
                Some(Rectangle::from_size(icon_size.to_f64())),
                Some(size),
                Kind::Unspecified,
            ) {
                out.push(PzElement::Memory(elem));
            }
        }
    }

    let [r, g, b] = group_color(&app_id);
    let radius = state.config.appearance.corner_radius as f64 * cam.zoom;
    let uniforms = [
        (
            "u_fill",
            ShaderValue::F4([r * 0.35, g * 0.35, b * 0.35, 0.92]),
        ),
        ("u_stroke", ShaderValue::F4([r, g, b, 1.0])),
        ("u_radius", ShaderValue::F1((radius.max(4.0) * os) as f32)),
        ("u_border", ShaderValue::F1((2.0 * os) as f32)),
    ];
    let elem = state.render_cache.cards.entry(id).or_default();
    out.push(PzElement::Shader(
        elem.update(&sh.rect, phys_rect(rect, os), opacity as f32, &uniforms)
            .clone(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_fade_in_below_the_threshold() {
        assert_eq!(card_opacity(0.3, 0.5), 0.0);
        assert_eq!(card_opacity(0.3, 0.3), 0.0);
        assert!(card_opacity(0.3, 0.25) > 0.0 && card_opacity(0.3, 0.25) < 1.0);
        assert_eq!(card_opacity(0.3, 0.1), 1.0);
    }

    #[test]
    fn zero_turns_it_off() {
        assert_eq!(card_opacity(0.0, 0.01), 0.0);
    }
}
