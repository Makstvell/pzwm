//! The infinite canvas: camera math, short camera animations and the
//! magnetic edge snapping used while dragging windows.
//!
//! Canvas coordinates are logical pixels at zoom 1. A camera maps them to
//! output-local logical (screen) coordinates:
//! `screen = (canvas - camera.pos) * zoom`.

use std::time::{Duration, Instant};

use smithay::utils::{Logical, Point, Rectangle};

pub const WORKSPACES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Canvas point shown at the output's top-left corner.
    pub pos: Point<f64, Logical>,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            pos: Point::from((0.0, 0.0)),
            zoom: 1.0,
        }
    }
}

impl Camera {
    pub fn to_screen(self, p: Point<f64, Logical>) -> Point<f64, Logical> {
        (p - self.pos).upscale(self.zoom)
    }

    pub fn to_canvas(self, s: Point<f64, Logical>) -> Point<f64, Logical> {
        self.pos + s.downscale(self.zoom)
    }

    /// The canvas area visible in `screen` (output-local logical rect).
    pub fn visible(&self, screen: Rectangle<f64, Logical>) -> Rectangle<f64, Logical> {
        Rectangle::new(self.to_canvas(screen.loc), screen.size.downscale(self.zoom))
    }

    /// Changes zoom while keeping the canvas point under `anchor` (screen
    /// coordinates) fixed — the "zoom to cursor" of design tools.
    pub fn zoomed_at(&self, anchor: Point<f64, Logical>, zoom: f64) -> Camera {
        let fixed = self.to_canvas(anchor);
        Camera {
            pos: fixed - anchor.downscale(zoom),
            zoom,
        }
    }

    /// Camera that centers `rect` (canvas) inside `screen` at `zoom`.
    pub fn centered_on(
        rect: Rectangle<f64, Logical>,
        screen: Rectangle<f64, Logical>,
        zoom: f64,
    ) -> Camera {
        let center = rect.loc + rect.size.downscale(2.0).to_point();
        let screen_center = screen.loc + screen.size.downscale(2.0).to_point();
        Camera {
            pos: center - screen_center.downscale(zoom),
            zoom,
        }
    }

    /// Camera that fits `rect` inside `screen` with `margin` screen pixels.
    pub fn fitting(
        rect: Rectangle<f64, Logical>,
        screen: Rectangle<f64, Logical>,
        margin: f64,
        min_zoom: f64,
        max_zoom: f64,
    ) -> Camera {
        let avail_w = (screen.size.w - 2.0 * margin).max(1.0);
        let avail_h = (screen.size.h - 2.0 * margin).max(1.0);
        let zoom = (avail_w / rect.size.w.max(1.0))
            .min(avail_h / rect.size.h.max(1.0))
            .clamp(min_zoom, max_zoom);
        Camera::centered_on(rect, screen, zoom)
    }

    /// Interpolates position linearly in screen space and zoom geometrically,
    /// which keeps zoom animations feeling even.
    fn lerp(a: Camera, b: Camera, t: f64) -> Camera {
        let zoom = a.zoom * (b.zoom / a.zoom).powf(t);
        // Interpolate the screen-center point so the motion does not swing.
        let pos = a.pos + (b.pos - a.pos).upscale(t);
        Camera { pos, zoom }
    }
}

/// Fast-out curve: quick response, short settle, no overshoot.
pub fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Debug, Clone, Copy)]
struct CameraAnim {
    from: Camera,
    to: Camera,
    start: Instant,
    duration: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Workspace {
    camera: Camera,
    anim: Option<CameraAnim>,
}

impl Workspace {
    pub fn camera(&self, now: Instant) -> Camera {
        match self.anim {
            Some(a) => {
                let t = now.saturating_duration_since(a.start).as_secs_f64()
                    / a.duration.as_secs_f64().max(1e-6);
                if t >= 1.0 {
                    a.to
                } else {
                    Camera::lerp(a.from, a.to, ease_out(t))
                }
            }
            None => self.camera,
        }
    }

    /// Where the camera is heading (equals `camera` when idle).
    pub fn target(&self) -> Camera {
        self.anim.map(|a| a.to).unwrap_or(self.camera)
    }

    pub fn set(&mut self, camera: Camera) {
        self.camera = camera;
        self.anim = None;
    }

    pub fn animate_to(&mut self, camera: Camera, duration: Duration, now: Instant) {
        if duration.is_zero() {
            self.set(camera);
            return;
        }
        let from = self.camera(now);
        self.camera = camera;
        self.anim = Some(CameraAnim {
            from,
            to: camera,
            start: now,
            duration,
        });
    }

    /// Drops a finished animation. Returns true while one is running.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.anim {
            Some(a) if now.saturating_duration_since(a.start) < a.duration => true,
            Some(_) => {
                self.anim = None;
                false
            }
            None => false,
        }
    }
}

/// Snaps a moving rectangle's edges to nearby target edges.
///
/// `rect` is the frame being dragged, `targets` other frames and the visible
/// area, `threshold` the snap distance in the same (canvas) units. Returns the
/// corrected top-left. Edges snap both outside (window next to window, with
/// `gap`) and inside (aligned edges).
pub fn snap_position(
    rect: Rectangle<f64, Logical>,
    targets: &[Rectangle<f64, Logical>],
    threshold: f64,
    gap: f64,
) -> Point<f64, Logical> {
    let mut best_dx: Option<f64> = None;
    let mut best_dy: Option<f64> = None;
    let consider = |best: &mut Option<f64>, delta: f64| {
        if delta.abs() <= threshold && best.is_none_or(|b| delta.abs() < b.abs()) {
            *best = Some(delta);
        }
    };

    let (l, r) = (rect.loc.x, rect.loc.x + rect.size.w);
    let (t, b) = (rect.loc.y, rect.loc.y + rect.size.h);
    for target in targets {
        let (tl, tr) = (target.loc.x, target.loc.x + target.size.w);
        let (tt, tb) = (target.loc.y, target.loc.y + target.size.h);
        // Neighbours separated only by the gap still count as touching.
        let reach = threshold + gap;
        let overlap_y = b + reach > tt && t - reach < tb;
        let overlap_x = r + reach > tl && l - reach < tr;
        if overlap_y {
            consider(&mut best_dx, tl - l); // left edges aligned
            consider(&mut best_dx, tr - r); // right edges aligned
            consider(&mut best_dx, tr + gap - l); // right of target
            consider(&mut best_dx, tl - gap - r); // left of target
        }
        if overlap_x {
            consider(&mut best_dy, tt - t);
            consider(&mut best_dy, tb - b);
            consider(&mut best_dy, tb + gap - t);
            consider(&mut best_dy, tt - gap - b);
        }
    }
    Point::from((
        rect.loc.x + best_dx.unwrap_or(0.0),
        rect.loc.y + best_dy.unwrap_or(0.0),
    ))
}

/// Bounding box of all `rects`, or `None` when empty.
pub fn bounding_box(rects: &[Rectangle<f64, Logical>]) -> Option<Rectangle<f64, Logical>> {
    rects.iter().copied().reduce(|a, b| a.merge(b))
}

/// Grid level for the background: spacing grows ×4 as the user zooms out
/// so dots never become noise. Returns (spacing, minor-dot fade 0..1).
pub fn grid_level(base: f64, zoom: f64) -> (f64, f64) {
    const MIN_PX: f64 = 10.0;
    const FULL_PX: f64 = 22.0;
    let mut spacing = base.max(1.0);
    for _ in 0..8 {
        if spacing * zoom >= MIN_PX {
            break;
        }
        spacing *= 4.0;
    }
    let px = spacing * zoom;
    let fade = ((px - MIN_PX) / (FULL_PX - MIN_PX)).clamp(0.0, 1.0);
    (spacing, fade)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::utils::Size;

    fn pt(x: f64, y: f64) -> Point<f64, Logical> {
        Point::from((x, y))
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new(pt(x, y), Size::from((w, h)))
    }

    #[test]
    fn screen_and_canvas_roundtrip() {
        let cam = Camera {
            pos: pt(100.0, -50.0),
            zoom: 0.5,
        };
        let p = pt(321.0, 77.0);
        let back = cam.to_canvas(cam.to_screen(p));
        assert!((back.x - p.x).abs() < 1e-9 && (back.y - p.y).abs() < 1e-9);
    }

    #[test]
    fn zoom_keeps_point_under_anchor() {
        let cam = Camera {
            pos: pt(10.0, 20.0),
            zoom: 1.0,
        };
        let anchor = pt(400.0, 300.0);
        let before = cam.to_canvas(anchor);
        let zoomed = cam.zoomed_at(anchor, 2.5);
        let after = zoomed.to_canvas(anchor);
        assert!((before.x - after.x).abs() < 1e-9 && (before.y - after.y).abs() < 1e-9);
        assert_eq!(zoomed.zoom, 2.5);
    }

    #[test]
    fn fitting_shows_whole_rect() {
        let screen = rect(0.0, 0.0, 1000.0, 500.0);
        let target = rect(-500.0, 0.0, 3000.0, 400.0);
        let cam = Camera::fitting(target, screen, 20.0, 0.01, 4.0);
        let visible = cam.visible(screen);
        assert!(visible.loc.x <= target.loc.x && visible.loc.y <= target.loc.y);
        assert!(visible.loc.x + visible.size.w >= target.loc.x + target.size.w);
        assert!(visible.loc.y + visible.size.h >= target.loc.y + target.size.h);
    }

    #[test]
    fn animation_reaches_target_and_stops() {
        let start = Instant::now();
        let mut ws = Workspace::default();
        let to = Camera {
            pos: pt(500.0, 0.0),
            zoom: 2.0,
        };
        ws.animate_to(to, Duration::from_millis(100), start);
        let mid = ws.camera(start + Duration::from_millis(50));
        assert!(mid.pos.x > 0.0 && mid.pos.x < 500.0);
        assert!(ws.tick(start + Duration::from_millis(50)));
        assert_eq!(ws.camera(start + Duration::from_millis(150)), to);
        assert!(!ws.tick(start + Duration::from_millis(150)));
        assert_eq!(ws.target(), to);
    }

    #[test]
    fn ease_out_is_monotonic_and_bounded() {
        let mut prev = 0.0;
        for i in 0..=20 {
            let v = ease_out(i as f64 / 20.0);
            assert!(v >= prev && (0.0..=1.0).contains(&v));
            prev = v;
        }
        assert_eq!(ease_out(1.0), 1.0);
    }

    #[test]
    fn snaps_next_to_neighbour_with_gap() {
        let neighbour = rect(0.0, 0.0, 100.0, 100.0);
        let moving = rect(113.0, 5.0, 50.0, 50.0);
        let snapped = snap_position(moving, &[neighbour], 8.0, 10.0);
        assert_eq!(snapped.x, 110.0);
        // Top edges are 5 apart, within threshold: aligned too.
        assert_eq!(snapped.y, 0.0);
    }

    #[test]
    fn does_not_snap_when_far() {
        let neighbour = rect(0.0, 0.0, 100.0, 100.0);
        let moving = rect(300.0, 300.0, 50.0, 50.0);
        assert_eq!(
            snap_position(moving, &[neighbour], 8.0, 10.0),
            pt(300.0, 300.0)
        );
    }

    #[test]
    fn grid_spacing_grows_when_zoomed_out() {
        let (near, fade_near) = grid_level(32.0, 1.0);
        assert_eq!(near, 32.0);
        assert_eq!(fade_near, 1.0);
        let (far, _) = grid_level(32.0, 0.05);
        assert!(far * 0.05 >= 10.0);
    }

    #[test]
    fn bounding_box_merges() {
        assert!(bounding_box(&[]).is_none());
        let bb = bounding_box(&[rect(0.0, 0.0, 10.0, 10.0), rect(20.0, -5.0, 5.0, 5.0)]).unwrap();
        assert_eq!(bb, rect(0.0, -5.0, 25.0, 15.0));
    }
}
