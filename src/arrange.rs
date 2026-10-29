//! On-demand layouts: `arrange grid|row|column` lines up the focused
//! window's group (or, outside a group, every canvas window of the
//! workspace) once and fits the camera to the result. Tiling stays an
//! action, never a mode: windows can be moved freely afterwards.

use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::{
    canvas::{Camera, bounding_box},
    shell::Placement,
    state::State,
};

/// Space around the arranged windows when the camera fits them.
const FIT_MARGIN: f64 = 48.0;
/// Windows whose tops are this close (canvas units) count as one row
/// when the current order is read.
const ROW_BAND: f64 = 200.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    /// Rows of about √n windows.
    Grid,
    Row,
    Column,
}

impl Arrangement {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "" | "grid" => Some(Self::Grid),
            "row" | "horizontal" => Some(Self::Row),
            "column" | "vertical" => Some(Self::Column),
            _ => None,
        }
    }

    fn per_row(self, n: usize) -> usize {
        match self {
            Self::Grid => (n as f64).sqrt().ceil().max(1.0) as usize,
            Self::Row => n.max(1),
            Self::Column => 1,
        }
    }
}

/// Top-left corners for frames of `sizes`, in the given order, laid out
/// from `origin` with `gap` between them. Each row is as tall as its
/// tallest frame.
pub fn layout(
    how: Arrangement,
    sizes: &[Size<f64, Logical>],
    origin: Point<f64, Logical>,
    gap: f64,
) -> Vec<Point<f64, Logical>> {
    let per_row = how.per_row(sizes.len());
    let mut out = Vec::with_capacity(sizes.len());
    let mut y = origin.y;
    for row in sizes.chunks(per_row) {
        let mut x = origin.x;
        for size in row {
            out.push(Point::from((x, y)));
            x += size.w + gap;
        }
        y += row.iter().map(|s| s.h).fold(0.0, f64::max) + gap;
    }
    out
}

/// Reading order: top to bottom, then left to right, with frames whose
/// tops are within `band` of each other counted as one row.
fn reading_order(frames: &[Rectangle<f64, Logical>], band: f64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..frames.len()).collect();
    let row = |y: f64| (y / band).round();
    order.sort_by(|&a, &b| {
        let (fa, fb) = (frames[a].loc, frames[b].loc);
        row(fa.y).total_cmp(&row(fb.y)).then(fa.x.total_cmp(&fb.x))
    });
    order
}

impl State {
    /// Windows `arrange` moves: the focused window's group, else every
    /// canvas window of the workspace.
    fn arrange_targets(&self, ws: usize) -> Vec<usize> {
        let group = self
            .focused_index()
            .and_then(|i| self.windows[i].group.clone());
        (0..self.windows.len())
            .filter(|&i| {
                let m = &self.windows[i];
                m.ws == ws
                    && self.shown(m)
                    && !m.scratchpad
                    && m.placement == Placement::Canvas
                    && (group.is_none() || m.group == group)
            })
            .collect()
    }

    pub fn arrange(&mut self, how: Arrangement) -> Result<String, String> {
        let output = self.active_output().ok_or("no output")?;
        let ws = self.output_ws(&output);
        let targets = self.arrange_targets(ws);
        if targets.is_empty() {
            return Err("no windows to arrange".into());
        }
        let frames: Vec<Rectangle<f64, Logical>> = targets
            .iter()
            .map(|&i| self.canvas_frame(&self.windows[i]))
            .collect();
        let origin = bounding_box(&frames).map(|b| b.loc).unwrap_or_default();
        let gap = self.config.appearance.gaps.max(0) as f64 * 2.0;
        let order = reading_order(&frames, ROW_BAND);
        let sizes: Vec<Size<f64, Logical>> = order.iter().map(|&k| frames[k].size).collect();
        let spots = layout(how, &sizes, origin, gap);

        let mut placed = Vec::with_capacity(spots.len());
        for (&k, spot) in order.iter().zip(spots) {
            let idx = targets[k];
            let title = self.title_height(&self.windows[idx]);
            self.windows[idx].pos = Point::from((spot.x, spot.y + title));
            placed.push(Rectangle::new(spot, frames[k].size));
        }

        if let Some(bbox) = bounding_box(&placed) {
            let area = self.usable_area(&output).to_f64();
            let canvas = &self.config.canvas;
            let cam = Camera::fitting(
                bbox,
                area,
                FIT_MARGIN,
                canvas.min_zoom,
                canvas.max_zoom.min(1.0),
            );
            self.remember_camera(&output);
            self.fly_to(&output, cam);
        }
        self.queue_redraw_all();
        Ok(format!("{} windows arranged", targets.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(w: f64, h: f64) -> Size<f64, Logical> {
        Size::from((w, h))
    }

    #[test]
    fn grid_wraps_after_the_square_root() {
        let sizes = vec![size(100.0, 50.0); 5];
        let spots = layout(Arrangement::Grid, &sizes, Point::from((0.0, 0.0)), 10.0);
        // 5 windows: rows of 3.
        assert_eq!(spots[0], Point::from((0.0, 0.0)));
        assert_eq!(spots[2], Point::from((220.0, 0.0)));
        assert_eq!(spots[3], Point::from((0.0, 60.0)));
    }

    #[test]
    fn rows_are_as_tall_as_their_tallest_window() {
        let sizes = [size(10.0, 30.0), size(10.0, 80.0), size(10.0, 10.0)];
        let spots = layout(Arrangement::Column, &sizes, Point::from((5.0, 5.0)), 0.0);
        assert_eq!(spots[1].y, 35.0);
        assert_eq!(spots[2].y, 115.0);
        assert!(spots.iter().all(|p| p.x == 5.0));
    }

    #[test]
    fn row_keeps_everything_on_one_line() {
        let sizes = vec![size(50.0, 50.0); 4];
        let spots = layout(Arrangement::Row, &sizes, Point::from((0.0, 0.0)), 0.0);
        assert!(spots.iter().all(|p| p.y == 0.0));
        assert_eq!(spots[3].x, 150.0);
    }

    #[test]
    fn reading_order_goes_by_rows_then_columns() {
        let r = |x: f64, y: f64| Rectangle::new(Point::from((x, y)), size(10.0, 10.0));
        let frames = [r(500.0, 0.0), r(0.0, 900.0), r(0.0, 30.0)];
        assert_eq!(reading_order(&frames, ROW_BAND), vec![2, 0, 1]);
    }

    #[test]
    fn parses_names() {
        assert_eq!(Arrangement::parse(""), Some(Arrangement::Grid));
        assert_eq!(Arrangement::parse("column"), Some(Arrangement::Column));
        assert_eq!(Arrangement::parse("spiral"), None);
    }
}
