//! Wallpaper parallax: the background layer is zoomed in a little, shifts
//! against the camera while the canvas is panned and, with `swing`, drifts
//! slowly left and right on its own.

use std::{f64::consts::TAU, time::Duration};

use smithay::{
    backend::renderer::element::{
        AsRenderElements, surface::WaylandSurfaceRenderElement, utils::RescaleRenderElement,
    },
    desktop::layer_map_for_output,
    output::Output,
    utils::{Logical, Point, Scale, Size},
    wayland::shell::wlr_layer::Layer,
};

use super::{PzElement, PzRenderer};
use crate::{config::Parallax, state::State};

/// Largest travel allowed, as a share of the screen width.
const MAX_STRENGTH: f64 = 0.15;
/// Shortest swing; anything faster stops looking calm.
const MIN_PERIOD_SECS: f64 = 5.0;
/// Panning this many screen widths moves the wallpaper most of its travel.
const CAMERA_SPAN: f64 = 3.0;
/// Share of the travel driven by the drift; the rest follows the camera.
const DRIFT_SHARE: f64 = 0.65;
/// Vertical sway relative to the horizontal one: mostly left and right.
const VERTICAL_SHARE: f64 = 0.25;

/// Zoom and shift (logical pixels) of the wallpaper `secs` into the drift,
/// with the camera's left edge at `camera_x`.
pub fn placement(
    cfg: &Parallax,
    secs: f64,
    camera_x: f64,
    screen: Size<f64, Logical>,
) -> (f64, Point<f64, Logical>) {
    let strength = cfg.strength.clamp(0.0, MAX_STRENGTH);
    // Zooming by `strength` leaves this much spare image on every side.
    let room_x = screen.w * strength / 2.0;
    let room_y = screen.h * strength / 2.0;

    let phase = TAU * secs / cfg.period.max(MIN_PERIOD_SECS);
    // Two sines at unrelated rates read as a slow, organic sway rather
    // than a metronome; their weights keep the sum within [-1, 1].
    let drift = 0.8 * phase.sin() + 0.2 * (0.37 * phase + 1.3).sin();
    let follows = cfg.follow_camera && screen.w > 0.0;
    let follow = if follows {
        (-camera_x / (screen.w * CAMERA_SPAN)).tanh()
    } else {
        0.0
    };
    let drift_share = match (cfg.swing, follows) {
        (true, true) => DRIFT_SHARE,
        (true, false) => 1.0,
        (false, _) => 0.0,
    };
    let x = (drift * drift_share + follow * (1.0 - drift_share)) * room_x;
    let y = if cfg.swing {
        VERTICAL_SHARE * (0.61 * phase + 0.7).sin() * room_y
    } else {
        0.0
    };
    (1.0 + strength, Point::from((x, y)))
}

/// Background layer surfaces of `output`, zoomed and shifted for parallax.
pub fn background_elements<R: PzRenderer>(
    state: &mut State,
    renderer: &mut R,
    output: &Output,
    os: f64,
    out: &mut Vec<PzElement<R>>,
) {
    let screen = state.output_geometry(output).size.to_f64();
    let secs = Duration::from(state.clock.now()).as_secs_f64();
    let camera_x = state.camera(output).pos.x;
    let (zoom, shift) = placement(&state.config.parallax, secs, camera_x, screen);
    let center =
        (Point::from((screen.w / 2.0, screen.h / 2.0)) + shift).to_physical_precise_round(os);

    let map = layer_map_for_output(output);
    for surface in map.layers_on(Layer::Background).rev() {
        let Some(geo) = map.layer_geometry(surface) else {
            continue;
        };
        let loc = (geo.loc.to_f64() + shift).to_physical_precise_round(os);
        let elems = AsRenderElements::<R>::render_elements::<WaylandSurfaceRenderElement<R>>(
            surface,
            renderer,
            loc,
            Scale::from(os),
            1.0,
        );
        out.extend(
            elems
                .into_iter()
                .map(|e| PzElement::Zoomed(RescaleRenderElement::from_element(e, center, zoom))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(follow_camera: bool) -> Parallax {
        Parallax {
            enabled: true,
            strength: 0.04,
            swing: true,
            period: 60.0,
            follow_camera,
        }
    }

    fn screen() -> Size<f64, Logical> {
        Size::from((1707.0, 960.0))
    }

    #[test]
    fn never_shows_an_edge() {
        for step in 0..2000 {
            let secs = step as f64 * 0.37;
            let camera_x = (step as f64 - 1000.0) * 97.0;
            let (zoom, shift) = placement(&cfg(true), secs, camera_x, screen());
            let room_x = screen().w * (zoom - 1.0) / 2.0;
            let room_y = screen().h * (zoom - 1.0) / 2.0;
            assert!(shift.x.abs() <= room_x + 1e-9, "x {} > {room_x}", shift.x);
            assert!(shift.y.abs() <= room_y + 1e-9, "y {} > {room_y}", shift.y);
        }
    }

    #[test]
    fn drifts_over_time() {
        let (_, a) = placement(&cfg(false), 0.0, 0.0, screen());
        let (_, b) = placement(&cfg(false), 15.0, 0.0, screen());
        assert!((a.x - b.x).abs() > 10.0);
    }

    #[test]
    fn moves_against_the_camera() {
        let (_, left) = placement(&cfg(true), 0.0, -2000.0, screen());
        let (_, right) = placement(&cfg(true), 0.0, 2000.0, screen());
        assert!(left.x > right.x);
    }

    #[test]
    fn without_swing_only_the_camera_moves_it() {
        let still = Parallax {
            swing: false,
            ..cfg(true)
        };
        let (_, a) = placement(&still, 0.0, 500.0, screen());
        let (_, b) = placement(&still, 15.0, 500.0, screen());
        assert_eq!(a, b);
        assert_eq!(a.y, 0.0);
        let (_, panned) = placement(&still, 0.0, -500.0, screen());
        assert!(panned.x > a.x);
    }

    #[test]
    fn clamps_strength() {
        let wild = Parallax {
            strength: 5.0,
            ..cfg(true)
        };
        let (zoom, _) = placement(&wild, 0.0, 0.0, screen());
        assert!((zoom - 1.0 - MAX_STRENGTH).abs() < 1e-9);
    }
}
