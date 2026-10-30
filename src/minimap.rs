// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! The minimap: a small map of the whole workspace with a live preview of
//! every canvas window and the visible area, so nothing gets lost on the
//! infinite canvas. It either sits in a screen corner or, with
//! `position = "cursor"`, is called up under the pointer and closes again.
//! Clicking a window jumps to it; clicking or dragging anywhere else moves
//! the view there.

use std::time::{Duration, Instant};

use smithay::{
    backend::input::Keycode,
    output::Output,
    utils::{Logical, Point, Rectangle, Size},
};

use crate::{
    canvas::{Camera, bounding_box},
    grabs::Interaction,
    shell::Placement,
    state::State,
};

/// Space between the map and the screen edge, logical pixels.
const MARGIN: f64 = 16.0;
/// Inner padding of the map, logical pixels.
const PADDING: f64 = 8.0;
/// Smallest drawn window, so tiny windows stay clickable.
const MIN_WINDOW_PX: f64 = 4.0;
/// Fade-in of the map called up at the cursor.
const POPUP_FADE: Duration = Duration::from_millis(120);
/// Holding the key at least this long makes its release close the map;
/// a shorter tap leaves it open.
const POPUP_HOLD: Duration = Duration::from_millis(300);

/// The map called up at the cursor.
#[derive(Debug, Clone)]
pub struct MapPopup {
    pub output: Output,
    /// Cursor position when it was called, output-local.
    pub anchor: Point<f64, Logical>,
    pub opened: Instant,
    /// Key that called it up; its release closes the map after a hold.
    pub key: Option<Keycode>,
}

/// Where the map sits on screen and how canvas space maps into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapLayout {
    /// The map panel, output-local screen coordinates.
    pub rect: Rectangle<f64, Logical>,
    /// Canvas point drawn at `offset`.
    origin: Point<f64, Logical>,
    /// Content origin on screen (panel + padding + centering).
    offset: Point<f64, Logical>,
    /// Screen pixels per canvas unit.
    scale: f64,
}

impl MapLayout {
    /// Fits `world` (canvas) into `panel` (screen) with padding.
    pub fn fit(panel: Rectangle<f64, Logical>, world: Rectangle<f64, Logical>) -> Self {
        let inner_w = (panel.size.w - 2.0 * PADDING).max(1.0);
        let inner_h = (panel.size.h - 2.0 * PADDING).max(1.0);
        let scale = (inner_w / world.size.w.max(1.0)).min(inner_h / world.size.h.max(1.0));
        let used_w = world.size.w * scale;
        let used_h = world.size.h * scale;
        let offset = panel.loc
            + Point::from((
                PADDING + (inner_w - used_w) / 2.0,
                PADDING + (inner_h - used_h) / 2.0,
            ));
        Self {
            rect: panel,
            origin: world.loc,
            offset,
            scale,
        }
    }

    pub fn to_map(self, canvas: Rectangle<f64, Logical>) -> Rectangle<f64, Logical> {
        let loc = self.offset + (canvas.loc - self.origin).upscale(self.scale);
        let size = Size::from((
            (canvas.size.w * self.scale).max(MIN_WINDOW_PX),
            (canvas.size.h * self.scale).max(MIN_WINDOW_PX),
        ));
        Rectangle::new(loc, size)
    }

    pub fn to_canvas(self, screen: Point<f64, Logical>) -> Point<f64, Logical> {
        self.origin + (screen - self.offset).downscale(self.scale)
    }

    pub fn point_to_map(self, canvas: Point<f64, Logical>) -> Point<f64, Logical> {
        self.offset + (canvas - self.origin).upscale(self.scale)
    }

    /// Screen pixels per canvas unit.
    pub fn scale(self) -> f64 {
        self.scale
    }
}

/// `size` centered on `anchor`, pushed inside `area` (minus `margin`).
fn place_around(
    anchor: Point<f64, Logical>,
    size: Size<f64, Logical>,
    area: Rectangle<f64, Logical>,
    margin: f64,
) -> Rectangle<f64, Logical> {
    let clamp = |v: f64, lo: f64, len: f64, span: f64| {
        v.clamp(lo + margin, (lo + span - len - margin).max(lo + margin))
    };
    let x = clamp(anchor.x - size.w / 2.0, area.loc.x, size.w, area.size.w);
    let y = clamp(anchor.y - size.h / 2.0, area.loc.y, size.h, area.size.h);
    Rectangle::new(Point::from((x, y)), size)
}

/// What a click on the map hits.
pub enum MapHit {
    Window(usize),
    Canvas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapKey {
    Panel,
    Window(u64),
    Viewport,
}

/// One rounded rectangle of the map, in output-local screen coordinates.
pub struct MapShape {
    pub key: MapKey,
    /// The window drawn as a preview under this shape.
    pub window: Option<usize>,
    pub rect: Rectangle<f64, Logical>,
    pub fill: [f32; 4],
    pub stroke: [f32; 4],
}

fn with_alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * a]
}

impl State {
    fn map_at_cursor(&self) -> bool {
        self.config.minimap.position == "cursor"
    }

    /// The "minimap" action: call up / dismiss the map at the cursor, or
    /// show / hide the corner map.
    pub fn toggle_minimap(&mut self) {
        if !self.map_at_cursor() {
            self.config.minimap.enabled = !self.config.minimap.enabled;
            self.queue_redraw_all();
            return;
        }
        if self.map_popup.is_some() {
            self.close_map_popup();
            return;
        }
        if !self.config.minimap.enabled {
            return;
        }
        let Some(output) = self.active_output() else {
            return;
        };
        self.map_popup = Some(MapPopup {
            anchor: self.cursor_local(&output),
            output: output.clone(),
            opened: Instant::now(),
            key: None,
        });
        self.queue_redraw(&output);
    }

    pub fn close_map_popup(&mut self) {
        if let Some(popup) = self.map_popup.take() {
            self.queue_redraw(&popup.output);
        }
    }

    /// Release of the key that called the map: closes it after a hold.
    pub fn map_key_released(&mut self) {
        if self
            .map_popup
            .as_ref()
            .is_some_and(|p| p.opened.elapsed() >= POPUP_HOLD)
            && self.interaction.is_none()
        {
            self.close_map_popup();
        }
    }

    /// A map drag ended; the map at the cursor has done its job.
    pub fn map_drag_ended(&mut self) {
        if self.map_at_cursor() {
            self.close_map_popup();
        }
    }

    pub fn map_popup_fading(&self, now: Instant) -> bool {
        self.map_popup
            .as_ref()
            .is_some_and(|p| now.duration_since(p.opened) < POPUP_FADE)
    }

    /// Opacity of the map on `output`, for the fade-in.
    pub fn minimap_alpha(&self, output: &Output) -> f32 {
        match &self.map_popup {
            Some(p) if &p.output == output && self.config.animation.enabled => {
                let t = p.opened.elapsed().as_secs_f64() / POPUP_FADE.as_secs_f64();
                t.min(1.0) as f32
            }
            _ => 1.0,
        }
    }

    /// Canvas windows of the output's workspace with their canvas frames,
    /// topmost first.
    fn map_windows(&self, output: &Output) -> Vec<(usize, Rectangle<f64, Logical>)> {
        self.stacking(output)
            .into_iter()
            .filter(|&i| self.windows[i].placement == Placement::Canvas)
            .map(|i| (i, self.canvas_frame(&self.windows[i])))
            .collect()
    }

    /// The map's geometry, or `None` while it is hidden.
    pub fn minimap_layout(&self, output: &Output) -> Option<MapLayout> {
        let cfg = &self.config.minimap;
        if !cfg.enabled || self.lock.is_locked() {
            return None;
        }
        let popup = self
            .map_popup
            .as_ref()
            .filter(|p| &p.output == output)
            .map(|p| p.anchor);
        if self.map_at_cursor() && popup.is_none() {
            return None;
        }
        // Keep the map still while it is being dragged, or it would rescale
        // under the cursor as the view moves.
        if let Some(Interaction::MapDrag { output: o, layout }) = &self.interaction {
            if o == output {
                return Some(*layout);
            }
        }
        let fullscreen = self
            .windows
            .iter()
            .any(|m| m.placement == Placement::Fullscreen && self.view(m, output).is_some());
        if fullscreen {
            return None;
        }
        let windows = self.map_windows(output);
        // Called up on purpose, an empty map still shows where the view is.
        if windows.is_empty() && popup.is_none() {
            return None;
        }
        let area = self.usable_area(output).to_f64();
        let mut rects: Vec<Rectangle<f64, Logical>> = windows.iter().map(|(_, r)| *r).collect();
        rects.push(self.camera(output).visible(area));
        let world = bounding_box(&rects)?;

        if let Some(anchor) = popup {
            let w = cfg
                .popup_width
                .clamp(120.0, (area.size.w - 2.0 * MARGIN).max(120.0));
            let h = w * area.size.h / area.size.w.max(1.0);
            let panel = place_around(anchor, Size::from((w, h)), area, MARGIN);
            return Some(MapLayout::fit(panel, world));
        }
        let w = cfg.width.clamp(80.0, (area.size.w / 2.0).max(80.0));
        let h = w * area.size.h / area.size.w.max(1.0);
        let x = if cfg.position.ends_with("left") {
            area.loc.x + MARGIN
        } else {
            area.loc.x + area.size.w - w - MARGIN
        };
        let y = if cfg.position.starts_with("top") {
            area.loc.y + MARGIN
        } else {
            area.loc.y + area.size.h - h - MARGIN
        };
        let panel = Rectangle::new(Point::from((x, y)), Size::from((w, h)));
        Some(MapLayout::fit(panel, world))
    }

    pub fn minimap_hit(&self, output: &Output, local: Point<f64, Logical>) -> Option<MapHit> {
        let layout = self.minimap_layout(output)?;
        if !layout.rect.contains(local) {
            return None;
        }
        let window = self
            .map_windows(output)
            .into_iter()
            .find(|(_, frame)| layout.to_map(*frame).contains(local))
            .map(|(i, _)| MapHit::Window(i));
        Some(window.unwrap_or(MapHit::Canvas))
    }

    /// Centers the view on the canvas point under `local` on the map.
    pub fn minimap_jump(
        &mut self,
        output: &Output,
        layout: &MapLayout,
        local: Point<f64, Logical>,
        animate: bool,
    ) {
        let r = layout.rect;
        let local = Point::from((
            local.x.clamp(r.loc.x, r.loc.x + r.size.w),
            local.y.clamp(r.loc.y, r.loc.y + r.size.h),
        ));
        let target = Rectangle::new(layout.to_canvas(local), Size::default());
        let ws = self.output_ws(output);
        let zoom = self.workspace(ws).target().zoom;
        let area = self.usable_area(output).to_f64();
        let cam = Camera::centered_on(target, area, zoom);
        let dur = if animate {
            self.camera_duration()
        } else {
            Duration::ZERO
        };
        self.workspace_mut(ws).animate_to(cam, dur, Instant::now());
        self.queue_redraw(output);
    }

    /// Shapes to draw, front to back like the rest of the element list.
    pub fn minimap_shapes(&self, output: &Output) -> Vec<MapShape> {
        let Some(layout) = self.minimap_layout(output) else {
            return Vec::new();
        };
        let cfg = &self.config.minimap;
        let area = self.usable_area(output).to_f64();
        let visible = self.camera(output).visible(area);
        let hovered = match self.minimap_hit(output, self.cursor_local(output)) {
            Some(MapHit::Window(i)) if self.output_at(self.cursor).as_ref() == Some(output) => {
                Some(i)
            }
            _ => None,
        };
        let mut shapes = vec![MapShape {
            key: MapKey::Viewport,
            window: None,
            rect: layout
                .to_map(visible)
                .intersection(layout.rect)
                .unwrap_or_default(),
            fill: with_alpha(cfg.viewport_color, 0.08),
            stroke: cfg.viewport_color,
        }];
        let focused = self.focused_index();
        for (i, frame) in self.map_windows(output) {
            let color = if Some(i) == focused || Some(i) == hovered {
                cfg.focused_color
            } else {
                cfg.window_color
            };
            // The live preview shows through; hover only tints it.
            let fill = if Some(i) == hovered {
                with_alpha(cfg.focused_color, 0.18)
            } else {
                [0.0; 4]
            };
            shapes.push(MapShape {
                key: MapKey::Window(self.windows[i].id),
                window: Some(i),
                rect: layout.to_map(frame),
                fill,
                stroke: color,
            });
        }
        shapes.push(MapShape {
            key: MapKey::Panel,
            window: None,
            rect: layout.rect,
            fill: cfg.background,
            stroke: with_alpha(cfg.viewport_color, 0.35),
        });
        shapes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new(Point::from((x, y)), Size::from((w, h)))
    }

    #[test]
    fn fit_keeps_world_inside_panel() {
        let panel = rect(100.0, 100.0, 216.0, 116.0);
        let world = rect(-1000.0, -500.0, 4000.0, 1000.0);
        let mapped = MapLayout::fit(panel, world).to_map(world);
        assert!(mapped.loc.x >= panel.loc.x + PADDING - 1e-6);
        assert!(mapped.loc.x + mapped.size.w <= panel.loc.x + panel.size.w - PADDING + 1e-6);
        assert!(mapped.loc.y >= panel.loc.y);
        assert!(mapped.loc.y + mapped.size.h <= panel.loc.y + panel.size.h);
    }

    #[test]
    fn popup_stays_on_screen_near_the_edge() {
        let area = rect(0.0, 0.0, 1000.0, 600.0);
        let size = Size::from((400.0, 240.0));
        let panel = place_around(Point::from((990.0, 590.0)), size, area, MARGIN);
        assert_eq!(
            panel.loc,
            Point::from((1000.0 - 400.0 - MARGIN, 600.0 - 240.0 - MARGIN))
        );
        let centered = place_around(Point::from((500.0, 300.0)), size, area, MARGIN);
        assert_eq!(centered.loc, Point::from((300.0, 180.0)));
    }

    #[test]
    fn map_and_canvas_roundtrip() {
        let layout = MapLayout::fit(
            rect(0.0, 0.0, 200.0, 120.0),
            rect(-300.0, 50.0, 900.0, 600.0),
        );
        let p = Point::from((120.0, 40.0));
        let back = layout.to_map(Rectangle::new(
            layout.to_canvas(p),
            Size::from((500.0, 500.0)),
        ));
        assert!((back.loc.x - p.x).abs() < 1e-9 && (back.loc.y - p.y).abs() < 1e-9);
    }
}
