//! Moving around: camera (pan, zoom, overview, centering), focus by
//! direction or cycling, half-screen tiling and workspaces.

use std::time::{Duration, Instant};

use smithay::{
    output::Output,
    utils::{Logical, Point, Rectangle, Size},
};

use crate::{
    action::Dir,
    canvas::{Camera, bounding_box},
    shell::Placement,
    state::{State, output_state},
};

impl State {
    pub fn center_on(&mut self, idx: usize) {
        let m = &self.windows[idx];
        if m.placement != Placement::Canvas {
            return;
        }
        let Some(output) = self
            .outputs
            .iter()
            .find(|o| self.output_ws(o) == m.ws)
            .cloned()
        else {
            return;
        };
        let frame = self.canvas_frame(m);
        let area = self.usable_area(&output).to_f64();
        let ws = m.ws;
        let zoom = self.workspace(ws).target().zoom;
        let cam = Camera::centered_on(frame, area, zoom);
        let dur = self.camera_duration();
        self.workspace_mut(ws).animate_to(cam, dur, Instant::now());
        self.queue_redraw(&output);
    }

    /// Centers a canvas window and zooms so it fills the visible area.
    pub fn fit_window(&mut self, idx: usize) {
        let m = &self.windows[idx];
        if m.placement != Placement::Canvas {
            return;
        }
        let Some(output) = self
            .outputs
            .iter()
            .find(|o| self.output_ws(o) == m.ws)
            .cloned()
        else {
            return;
        };
        let frame = self.canvas_frame(m);
        let area = self.usable_area(&output).to_f64();
        let ws = m.ws;
        let canvas = &self.config.canvas;
        let margin = self.config.appearance.gaps as f64;
        let cam = Camera::fitting(frame, area, margin, canvas.min_zoom, canvas.max_zoom);
        let dur = self.camera_duration();
        self.workspace_mut(ws).animate_to(cam, dur, Instant::now());
        self.queue_redraw(&output);
    }

    /// Takes the camera to 100 % zoom, keeping the middle of the screen
    /// where it is, so what is placed next gets real-pixel sizes.
    pub fn camera_to_full_scale(&mut self, output: &Output) {
        let ws = self.output_ws(output);
        let cam = self.workspace(ws).target();
        if (cam.zoom - 1.0).abs() < 0.001 {
            return;
        }
        let center = self
            .output_geometry(output)
            .size
            .to_f64()
            .to_point()
            .downscale(2.0);
        let target = Camera {
            pos: cam.to_canvas(center) - center,
            zoom: 1.0,
        };
        self.remember_camera(output);
        let dur = self.camera_duration();
        self.workspace_mut(ws)
            .animate_to(target, dur, Instant::now());
        self.queue_redraw(output);
    }

    /// A canvas window shown at under `SMALL_SHARE` of the screen both ways
    /// at the camera's target zoom: too small to read.
    pub fn too_small_to_use(&self, idx: usize) -> bool {
        const SMALL_SHARE: f64 = 0.45;
        let m = &self.windows[idx];
        if m.placement != Placement::Canvas {
            return false;
        }
        let Some(output) = self.outputs.iter().find(|o| self.output_ws(o) == m.ws) else {
            return false;
        };
        let area = self.usable_area(output).size.to_f64();
        let shown = self
            .canvas_frame(m)
            .size
            .upscale(self.workspace(m.ws).target().zoom);
        shown.w < area.w * SMALL_SHARE && shown.h < area.h * SMALL_SHARE
    }

    /// Pans just enough to bring a canvas window fully into view.
    pub fn reveal(&mut self, idx: usize) {
        let m = &self.windows[idx];
        if m.placement != Placement::Canvas {
            return;
        }
        let Some(output) = self
            .outputs
            .iter()
            .find(|o| self.output_ws(o) == m.ws)
            .cloned()
        else {
            return;
        };
        let ws = m.ws;
        let frame = self.canvas_frame(m);
        let cam = self.workspace(ws).target();
        let visible = cam.visible(self.usable_area(&output).to_f64());
        let fits = frame.size.w <= visible.size.w && frame.size.h <= visible.size.h;
        let inside = visible.contains_rect(frame);
        if inside {
            return;
        }
        if fits {
            let margin = self.config.appearance.gaps as f64 * 2.0 / cam.zoom;
            let mut d = Point::<f64, Logical>::from((0.0, 0.0));
            if frame.loc.x < visible.loc.x {
                d.x = frame.loc.x - visible.loc.x - margin;
            } else if frame.loc.x + frame.size.w > visible.loc.x + visible.size.w {
                d.x = frame.loc.x + frame.size.w - visible.loc.x - visible.size.w + margin;
            }
            if frame.loc.y < visible.loc.y {
                d.y = frame.loc.y - visible.loc.y - margin;
            } else if frame.loc.y + frame.size.h > visible.loc.y + visible.size.h {
                d.y = frame.loc.y + frame.size.h - visible.loc.y - visible.size.h + margin;
            }
            let target = Camera {
                pos: cam.pos + d,
                zoom: cam.zoom,
            };
            let dur = self.camera_duration();
            self.workspace_mut(ws)
                .animate_to(target, dur, Instant::now());
            self.queue_redraw(&output);
        } else {
            self.center_on(idx);
        }
    }

    pub fn focus_dir(&mut self, dir: Dir) {
        let Some(output) = self.active_output() else {
            return;
        };
        let candidates: Vec<(usize, Point<f64, Logical>)> = self
            .stacking(&output)
            .into_iter()
            .filter_map(|i| {
                let view = self.view(&self.windows[i], &output)?;
                let f = self.frame_rect(&self.windows[i], &view);
                Some((i, f.loc + f.size.downscale(2.0).to_point()))
            })
            .collect();
        let from = self
            .focused_index()
            .and_then(|f| candidates.iter().find(|(i, _)| *i == f).map(|(_, c)| *c))
            .unwrap_or_else(|| self.cursor_local(&output));
        let (dx, dy) = dir.vector();
        let best = candidates
            .iter()
            .filter(|(i, _)| Some(*i) != self.focused_index())
            .filter_map(|(i, c)| {
                let d = *c - from;
                let along = d.x * dx + d.y * dy;
                if along <= 1.0 {
                    return None;
                }
                let across = (d.x * dy - d.y * dx).abs();
                Some((*i, along + across * 2.0))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = best {
            let window = self.windows[i].window.clone();
            self.focus_window(Some(window));
            if let Some(i) = self.index_of(&self.windows[0].window.clone()) {
                self.reveal(i);
            }
        }
    }

    /// Alt+Tab: next window of this workspace (by age), centered on screen.
    /// Ids of the active workspace's windows (and pinned ones) in the order
    /// they were opened.
    fn launch_order(&self) -> Vec<u64> {
        let Some(output) = self.active_output() else {
            return Vec::new();
        };
        let ws = self.output_ws(&output);
        let mut ids: Vec<u64> = self
            .windows
            .iter()
            .filter(|m| self.shown(m) && (m.ws == ws || m.placement == Placement::Pinned))
            .map(|m| m.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    pub fn focus_and_center(&mut self, id: u64) {
        if let Some(i) = self.index_of_id(id) {
            self.leave_fullscreen_for(i);
            let window = self.windows[i].window.clone();
            self.focus_window(Some(window));
            self.center_on(0);
        }
    }

    /// A fullscreen window would keep covering the one being jumped to, so
    /// it goes back to where it was (like Hyprland does).
    fn leave_fullscreen_for(&mut self, target: usize) {
        let ws = self.windows[target].ws;
        let covering: Vec<usize> = (0..self.windows.len())
            .filter(|&i| {
                i != target
                    && self.windows[i].ws == ws
                    && self.windows[i].placement == Placement::Fullscreen
            })
            .collect();
        for i in covering {
            self.restore(i);
        }
    }

    /// Jumps to the `n`-th window (0-based) by `[general] window_numbers`,
    /// see numbering.rs.
    pub fn focus_nth(&mut self, n: usize) {
        if let Some(&id) = self.numbered_windows().get(n) {
            self.focus_and_center(id);
        }
    }

    pub fn focus_cycle(&mut self, forward: bool) {
        let ids = self.launch_order();
        if ids.is_empty() {
            return;
        }
        let current = self.focused_index().map(|i| self.windows[i].id);
        let pos = current.and_then(|c| ids.iter().position(|&id| id == c));
        let next = match (pos, forward) {
            (Some(p), true) => ids[(p + 1) % ids.len()],
            (Some(p), false) => ids[(p + ids.len() - 1) % ids.len()],
            (None, _) => ids[0],
        };
        self.focus_and_center(next);
    }

    // ---- camera commands ---------------------------------------------------

    pub fn zoom_at(
        &mut self,
        output: &Output,
        anchor: Point<f64, Logical>,
        factor: f64,
        animate: bool,
    ) {
        let ws = self.output_ws(output);
        let canvas = &self.config.canvas;
        let base = self.workspace(ws).target();
        let zoom = (base.zoom * factor).clamp(canvas.min_zoom, canvas.max_zoom);
        let target = base.zoomed_at(anchor, zoom);
        let dur = if animate {
            self.camera_duration()
        } else {
            Duration::ZERO
        };
        self.workspace_mut(ws)
            .animate_to(target, dur, Instant::now());
        self.queue_redraw(output);
    }

    pub fn zoom_reset(&mut self, output: &Output) {
        let anchor = self.usable_area(output).to_f64();
        let center = anchor.loc + anchor.size.downscale(2.0).to_point();
        let ws = self.output_ws(output);
        let zoom = self.workspace(ws).target().zoom;
        self.zoom_at(output, center, 1.0 / zoom, true);
    }

    /// Pans by a screen-space delta.
    pub fn pan(&mut self, output: &Output, delta: Point<f64, Logical>, animate: bool) {
        let ws = self.output_ws(output);
        let now = Instant::now();
        let base = if animate {
            self.workspace(ws).target()
        } else {
            self.workspace(ws).camera(now)
        };
        let target = Camera {
            pos: base.pos - delta.downscale(base.zoom),
            zoom: base.zoom,
        };
        let dur = if animate {
            self.camera_duration()
        } else {
            Duration::ZERO
        };
        self.workspace_mut(ws).animate_to(target, dur, now);
        self.queue_redraw(output);
    }

    /// Zooms out to fit every window; a second press returns to where you were.
    pub fn overview(&mut self, output: &Output) {
        let ws = self.output_ws(output);
        let frames: Vec<Rectangle<f64, Logical>> = self
            .windows
            .iter()
            .filter(|m| self.shown(m) && m.ws == ws && m.placement == Placement::Canvas)
            .map(|m| self.canvas_frame(m))
            .collect();
        let Some(bbox) = bounding_box(&frames) else {
            return;
        };
        let area = self.usable_area(output).to_f64();
        let canvas = &self.config.canvas;
        let fit = Camera::fitting(bbox, area, 48.0, canvas.min_zoom, canvas.max_zoom.min(1.0));
        let current = self.workspace(ws).target();
        let target = match self.pre_overview.take() {
            Some((w, back)) if w == ws && current == fit => back,
            _ => {
                self.pre_overview = Some((ws, current));
                fit
            }
        };
        let dur = self.camera_duration();
        self.workspace_mut(ws)
            .animate_to(target, dur, Instant::now());
        self.queue_redraw(output);
    }

    pub fn home(&mut self, output: &Output) {
        let ws = self.output_ws(output);
        let dur = self.camera_duration();
        self.workspace_mut(ws)
            .animate_to(Camera::default(), dur, Instant::now());
        self.queue_redraw(output);
    }

    // ---- workspaces --------------------------------------------------------

    pub fn switch_workspace(&mut self, ws: usize) {
        let Some(output) = self.active_output() else {
            return;
        };
        let current = self.output_ws(&output);
        if current == ws {
            return;
        }
        // A workspace shows on one output at a time: swap if it is elsewhere.
        if let Some(other) = self
            .outputs
            .iter()
            .find(|o| *o != &output && self.output_ws(o) == ws)
        {
            output_state(other).ws = current;
        }
        output_state(&output).ws = ws;
        let next = self
            .windows
            .iter()
            .find(|m| self.shown(m) && m.ws == ws && m.placement != Placement::Pinned)
            .map(|m| m.window.clone());
        self.focus_window(next);
        self.queue_redraw_all();
    }

    pub fn move_to_workspace(&mut self, idx: usize, ws: usize) {
        let m = &mut self.windows[idx];
        if m.ws == ws || m.placement == Placement::Pinned {
            return;
        }
        m.ws = ws;
        let window = m.window.clone();
        if self.focused.as_ref() == Some(&window) {
            let current = self
                .active_output()
                .map(|o| self.output_ws(&o))
                .unwrap_or(0);
            let next = self
                .windows
                .iter()
                .find(|m| self.shown(m) && m.ws == current && m.window != window)
                .map(|m| m.window.clone());
            self.focus_window(next);
        }
        self.queue_redraw_all();
    }

    /// Tiles a canvas window to the left/right of what is on screen: a half,
    /// then a third and two thirds on repeated presses, see tile.rs.
    pub fn snap(&mut self, idx: usize, dir: Dir) {
        match dir {
            Dir::Up => return self.maximize(idx),
            Dir::Down => return self.restore(idx),
            Dir::Left | Dir::Right => {}
        }
        let Some(output) = self.output_of(idx) else {
            return;
        };
        if matches!(
            self.windows[idx].placement,
            Placement::Maximized | Placement::Fullscreen
        ) {
            self.restore(idx);
        }
        let rect = self.snap_share_rect(idx, &output, dir);
        self.place_in_screen_rect(idx, &output, rect);
    }

    /// Left/right half of the usable area, output-local screen coordinates.
    pub fn half_rect(&self, output: &Output, dir: Dir) -> Rectangle<f64, Logical> {
        let area = self.usable_area(output).to_f64();
        let gap = self.config.appearance.gaps as f64;
        let w = (area.size.w - 3.0 * gap) / 2.0;
        let x = match dir {
            Dir::Left => area.loc.x + gap,
            _ => area.loc.x + 2.0 * gap + w,
        };
        Rectangle::new(
            Point::from((x, area.loc.y + gap)),
            Size::from((w, area.size.h - 2.0 * gap)),
        )
    }

    /// Puts a window's frame onto a screen rectangle (tile grid, snap, edge
    /// and corner drops). By `[canvas] tile_mode`: "zoom" first takes the
    /// camera to 100 %, so the window gets the rectangle's size in real
    /// pixels whatever the zoom was; "pin" pins it there at 1:1; "canvas"
    /// keeps the zoom. A pinned window stays pinned there at 1:1.
    pub fn place_in_screen_rect(
        &mut self,
        idx: usize,
        output: &Output,
        rect: Rectangle<f64, Logical>,
    ) {
        if self.windows[idx].placement == Placement::Canvas {
            match self.config.canvas.tile_mode.as_str() {
                "canvas" => {}
                "pin" => {
                    let m = &mut self.windows[idx];
                    m.placement = Placement::Pinned;
                    m.output = Some(output.name());
                }
                _ => self.camera_to_full_scale(output),
            }
        }
        if self.windows[idx].placement == Placement::Pinned {
            let title = self.title_height(&self.windows[idx]);
            let m = &mut self.windows[idx];
            m.pos = Point::from((rect.loc.x, rect.loc.y + title));
            m.output = Some(output.name());
            let size = Size::from((
                rect.size.w.round() as i32,
                (rect.size.h - title).round().max(1.0) as i32,
            ));
            self.configure(idx, Some(size), false, false);
            self.queue_redraw_all();
            return;
        }
        let ws = self.output_ws(output);
        let cam = self.workspace(ws).target();
        let title = self.title_height(&self.windows[idx]);
        let canvas_rect = cam.visible(rect);
        let m = &mut self.windows[idx];
        m.placement = Placement::Canvas;
        m.output = None;
        m.ws = ws;
        m.pos = Point::from((canvas_rect.loc.x, canvas_rect.loc.y + title));
        let size = Size::from((
            canvas_rect.size.w.round() as i32,
            (canvas_rect.size.h - title).round().max(1.0) as i32,
        ));
        self.configure(idx, Some(size), false, false);
        self.queue_redraw_all();
    }
}
