// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Splits: windows tiled together over one rectangle of the canvas, in
//! columns that each hold one or more rows. They behave like one piece:
//! - drag the gap (or the shared edge) between two of them and both resize,
//!   like Windows' snapped windows; double-click it to share evenly; drag
//!   an outer edge and the whole split grows;
//! - `split` (`Super+S`) turns the focused window into one, filling the
//!   view; pressing it again lets the window go;
//! - while a new window waits for its place, an arrow puts it into the
//!   focused window's split on that side (a new column left or right, a
//!   new row above or below);
//! - windows snapped next to each other (halves, thirds, quarters, tile
//!   grid) join up into a split by themselves;
//! - a window that closes, is maximized, pinned or dragged away by its
//!   title leaves, and the others take its space.

use std::time::{Duration, Instant};

use smithay::{
    input::pointer::CursorIcon,
    output::Output,
    utils::{Logical, Point, Rectangle, Size},
};

use crate::{action::Dir, shell::Placement, state::State};

/// A column or row is never squeezed below this (canvas units).
const MIN_SPAN: f64 = 160.0;
/// How far from a gap's middle a press still grabs it (screen pixels).
const GRAB_PX: f64 = 10.0;
/// Two presses on the same divider this close together: a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Frames this close (canvas units) count as lined up when snapping.
const LINE_UP: f64 = 8.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub id: u64,
    pub weight: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub weight: f64,
    pub rows: Vec<Cell>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub id: u64,
    pub ws: usize,
    /// The whole area, canvas units (window frames, title bars included).
    pub rect: Rectangle<f64, Logical>,
    pub cols: Vec<Column>,
}

/// What a drag on a split moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// The gap after column `i`.
    Col(usize),
    /// The gap after row `i` of column `col`.
    Row { col: usize, i: usize },
    /// An outer edge of the whole split.
    Outer(Dir),
    /// The whole split, moved (`move-group` drag on a member).
    Whole,
}

/// Start and length of each of `weights` along a span of `len`, `gap` apart.
pub fn spans(start: f64, len: f64, weights: &[f64], gap: f64) -> Vec<(f64, f64)> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let total: f64 = weights.iter().sum::<f64>().max(1e-9);
    let free = (len - gap * (n - 1) as f64).max(0.0);
    let mut at = start;
    weights
        .iter()
        .map(|w| {
            let l = free * w / total;
            let s = (at, l);
            at += l + gap;
            s
        })
        .collect()
}

impl Split {
    pub fn new(id: u64, ws: usize, rect: Rectangle<f64, Logical>, window: u64) -> Self {
        Self {
            id,
            ws,
            rect,
            cols: vec![Column {
                weight: 1.0,
                rows: vec![Cell {
                    id: window,
                    weight: 1.0,
                }],
            }],
        }
    }

    pub fn members(&self) -> impl Iterator<Item = u64> + '_ {
        self.cols.iter().flat_map(|c| c.rows.iter().map(|r| r.id))
    }

    pub fn position(&self, id: u64) -> Option<(usize, usize)> {
        self.cols
            .iter()
            .enumerate()
            .find_map(|(c, col)| col.rows.iter().position(|r| r.id == id).map(|r| (c, r)))
    }

    /// Frame of every member, canvas units.
    pub fn layout(&self, gap: f64) -> Vec<(u64, Rectangle<f64, Logical>)> {
        let r = self.rect;
        let widths: Vec<f64> = self.cols.iter().map(|c| c.weight).collect();
        let xs = spans(r.loc.x, r.size.w, &widths, gap);
        let mut out = Vec::new();
        for (col, (x, w)) in self.cols.iter().zip(xs) {
            let heights: Vec<f64> = col.rows.iter().map(|c| c.weight).collect();
            for (cell, (y, h)) in col.rows.iter().zip(spans(r.loc.y, r.size.h, &heights, gap)) {
                out.push((cell.id, Rectangle::new((x, y).into(), (w, h).into())));
            }
        }
        out
    }

    /// Adds `new` next to `anchor` on `side`: a column of its own for left
    /// and right, a row in the anchor's column for up and down. It gets an
    /// even share; the others shrink in proportion.
    pub fn insert(&mut self, anchor: u64, new: u64, side: Dir) -> bool {
        let Some((c, r)) = self.position(anchor) else {
            return false;
        };
        match side {
            Dir::Left | Dir::Right => {
                let n = self.cols.len() as f64;
                let share: f64 = self.cols.iter().map(|c| c.weight).sum::<f64>() / n;
                let at = if side == Dir::Left { c } else { c + 1 };
                self.cols.insert(
                    at,
                    Column {
                        weight: share,
                        rows: vec![Cell {
                            id: new,
                            weight: 1.0,
                        }],
                    },
                );
            }
            Dir::Up | Dir::Down => {
                let rows = &mut self.cols[c].rows;
                let n = rows.len() as f64;
                let share: f64 = rows.iter().map(|r| r.weight).sum::<f64>() / n;
                let at = if side == Dir::Up { r } else { r + 1 };
                rows.insert(
                    at,
                    Cell {
                        id: new,
                        weight: share,
                    },
                );
            }
        }
        true
    }

    /// Takes `id` out; the neighbours take its share.
    pub fn remove(&mut self, id: u64) -> bool {
        let Some((c, r)) = self.position(id) else {
            return false;
        };
        self.cols[c].rows.remove(r);
        if self.cols[c].rows.is_empty() {
            self.cols.remove(c);
        }
        true
    }

    pub fn len(&self) -> usize {
        self.cols.iter().map(|c| c.rows.len()).sum()
    }

    /// The handle at canvas point `p` (`slop`: how close counts), if any.
    pub fn handle_at(&self, p: Point<f64, Logical>, gap: f64, slop: f64) -> Option<Handle> {
        let r = self.rect;
        let inside_y = p.y >= r.loc.y - slop && p.y <= r.loc.y + r.size.h + slop;
        let inside_x = p.x >= r.loc.x - slop && p.x <= r.loc.x + r.size.w + slop;
        if !(inside_x && inside_y) {
            return None;
        }
        let widths: Vec<f64> = self.cols.iter().map(|c| c.weight).collect();
        let xs = spans(r.loc.x, r.size.w, &widths, gap);
        for (i, &(x, w)) in xs.iter().enumerate().take(xs.len().saturating_sub(1)) {
            let mid = x + w + gap / 2.0;
            if (p.x - mid).abs() <= gap / 2.0 + slop {
                return Some(Handle::Col(i));
            }
        }
        for (c, (col, &(x, w))) in self.cols.iter().zip(&xs).enumerate() {
            if p.x < x || p.x > x + w {
                continue;
            }
            let heights: Vec<f64> = col.rows.iter().map(|c| c.weight).collect();
            let ys = spans(r.loc.y, r.size.h, &heights, gap);
            for (i, &(y, h)) in ys.iter().enumerate().take(ys.len().saturating_sub(1)) {
                let mid = y + h + gap / 2.0;
                if (p.y - mid).abs() <= gap / 2.0 + slop {
                    return Some(Handle::Row { col: c, i });
                }
            }
        }
        None
    }

    /// `base` with `handle` dragged by `delta` canvas units.
    pub fn dragged(base: &Split, handle: Handle, delta: Point<f64, Logical>, gap: f64) -> Split {
        let mut s = base.clone();
        match handle {
            Handle::Col(i) => {
                let widths: Vec<f64> = s.cols.iter().map(|c| c.weight).collect();
                let xs = spans(s.rect.loc.x, s.rect.size.w, &widths, gap);
                if let Some((a, b)) = shift_pair(xs[i].1, xs[i + 1].1, delta.x) {
                    let unit = (widths[i] + widths[i + 1]) / (xs[i].1 + xs[i + 1].1).max(1e-9);
                    s.cols[i].weight = a * unit;
                    s.cols[i + 1].weight = b * unit;
                }
            }
            Handle::Row { col, i } => {
                let heights: Vec<f64> = s.cols[col].rows.iter().map(|c| c.weight).collect();
                let ys = spans(s.rect.loc.y, s.rect.size.h, &heights, gap);
                if let Some((a, b)) = shift_pair(ys[i].1, ys[i + 1].1, delta.y) {
                    let unit = (heights[i] + heights[i + 1]) / (ys[i].1 + ys[i + 1].1).max(1e-9);
                    s.cols[col].rows[i].weight = a * unit;
                    s.cols[col].rows[i + 1].weight = b * unit;
                }
            }
            Handle::Whole => {
                s.rect.loc += delta;
            }
            Handle::Outer(dir) => {
                let r = base.rect;
                let (mut x0, mut y0) = (r.loc.x, r.loc.y);
                let (mut x1, mut y1) = (r.loc.x + r.size.w, r.loc.y + r.size.h);
                let min_w = MIN_SPAN * s.cols.len() as f64;
                let min_h =
                    MIN_SPAN * s.cols.iter().map(|c| c.rows.len()).max().unwrap_or(1) as f64;
                match dir {
                    Dir::Left => x0 = (x0 + delta.x).min(x1 - min_w),
                    Dir::Right => x1 = (x1 + delta.x).max(x0 + min_w),
                    Dir::Up => y0 = (y0 + delta.y).min(y1 - min_h),
                    Dir::Down => y1 = (y1 + delta.y).max(y0 + min_h),
                }
                s.rect = Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into());
            }
        }
        s
    }

    /// The two sides of a handle share evenly again (double click).
    pub fn even_out(&mut self, handle: Handle) {
        match handle {
            Handle::Col(i) => {
                let w = (self.cols[i].weight + self.cols[i + 1].weight) / 2.0;
                self.cols[i].weight = w;
                self.cols[i + 1].weight = w;
            }
            Handle::Row { col, i } => {
                let rows = &mut self.cols[col].rows;
                let w = (rows[i].weight + rows[i + 1].weight) / 2.0;
                rows[i].weight = w;
                rows[i + 1].weight = w;
            }
            Handle::Outer(_) | Handle::Whole => {}
        }
    }
}

/// Lengths `a`, `b` with `d` moved from `b` to `a`, both kept at least
/// `MIN_SPAN` (`None` when they already are as far as they go).
fn shift_pair(a: f64, b: f64, d: f64) -> Option<(f64, f64)> {
    let d = d.clamp(MIN_SPAN - a, b - MIN_SPAN);
    (a + b >= 2.0 * MIN_SPAN).then_some((a + d, b - d))
}

/// A divider press, for telling a double click.
#[derive(Clone, Copy, Debug)]
pub struct LastPress {
    pub split: u64,
    pub handle: Handle,
    pub at: Instant,
}

impl State {
    fn split_gap(&self) -> f64 {
        self.config.appearance.gaps as f64
    }

    pub fn split_index_of(&self, window: u64) -> Option<usize> {
        self.splits
            .iter()
            .position(|s| s.members().any(|m| m == window))
    }

    /// Puts every member where the split says.
    pub fn relayout_split(&mut self, si: usize) {
        let gap = self.split_gap();
        let frames = self.splits[si].layout(gap);
        let ws = self.splits[si].ws;
        for (id, frame) in frames {
            let Some(idx) = self.index_of_id(id) else {
                continue;
            };
            let title = self.title_height(&self.windows[idx]);
            let m = &mut self.windows[idx];
            m.ws = ws;
            m.placement = Placement::Canvas;
            m.output = None;
            m.pos = Point::from((frame.loc.x, frame.loc.y + title));
            let size = Size::from((
                frame.size.w.round().max(1.0) as i32,
                (frame.size.h - title).round().max(1.0) as i32,
            ));
            if (m.size().w - size.w as f64).abs() > 0.5 || (m.size().h - size.h as f64).abs() > 0.5
            {
                self.configure(idx, Some(size), false, false);
            }
        }
        self.queue_redraw_all();
    }

    /// `split`: the focused window becomes a split filling the view, or,
    /// when it is in one already, leaves it.
    pub fn toggle_split(&mut self) -> Result<String, String> {
        let idx = self.focused_index().ok_or("no window focused")?;
        let id = self.windows[idx].id;
        if self.split_index_of(id).is_some() {
            self.leave_split(id);
            return Ok("left the split".into());
        }
        if self.windows[idx].placement != Placement::Canvas {
            self.restore(idx);
        }
        let output = self.output_of(idx).ok_or("no output")?;
        if self.config.canvas.tile_mode == "zoom" {
            self.camera_to_full_scale(&output);
        }
        let ws = self.output_ws(&output);
        let gap = self.split_gap();
        let area = self.usable_area(&output).to_f64();
        let screen = Rectangle::new(
            (area.loc.x + gap, area.loc.y + gap).into(),
            (area.size.w - 2.0 * gap, area.size.h - 2.0 * gap).into(),
        );
        let rect = self.workspace(ws).target().visible(screen);
        let split = Split::new(self.next_split_id, ws, rect, id);
        self.next_split_id += 1;
        self.splits.push(split);
        let si = self.splits.len() - 1;
        self.relayout_split(si);
        Ok("split: new windows join it with an arrow while you pick their place".into())
    }

    /// Takes `window` out of its split; the others take its space, and a
    /// split of one is no split.
    pub fn leave_split(&mut self, window: u64) {
        let Some(si) = self.split_index_of(window) else {
            return;
        };
        self.splits[si].remove(window);
        if self.splits[si].len() <= 1 {
            self.splits.remove(si);
            return;
        }
        self.relayout_split(si);
    }

    /// The frame `new` gets when it joins `anchor`'s split on `side`;
    /// `None` when the anchor is in no split.
    pub fn join_split(
        &mut self,
        anchor: u64,
        new: u64,
        side: Dir,
    ) -> Option<Rectangle<f64, Logical>> {
        self.split_index_of(anchor)?;
        self.leave_split(new);
        let si = self.split_index_of(anchor)?;
        if !self.splits[si].insert(anchor, new, side) {
            return None;
        }
        let gap = self.split_gap();
        let frame = self.splits[si]
            .layout(gap)
            .into_iter()
            .find(|(id, _)| *id == new)
            .map(|(_, r)| r);
        // Everyone else moves now; the new window is placed by the caller.
        self.relayout_split(si);
        frame
    }

    /// After a window was snapped into a screen rectangle: when it lines up
    /// with a neighbour (same top and height side by side, or same left and
    /// width stacked), the two become a split, or it joins the neighbour's.
    pub fn link_snapped(&mut self, idx: usize) {
        let m = &self.windows[idx];
        if m.placement != Placement::Canvas {
            return;
        }
        let (id, ws) = (m.id, m.ws);
        let a = self.canvas_frame(m);
        let gap = self.split_gap();
        let reach = gap * 3.0 + LINE_UP;
        let close = |x: f64, y: f64| (x - y).abs() <= LINE_UP;
        let neighbour = self.windows.iter().find_map(|o| {
            if o.id == id || o.ws != ws || o.placement != Placement::Canvas || !o.mapped || o.hidden
            {
                return None;
            }
            let b = self.canvas_frame(o);
            let side_by_side = close(a.loc.y, b.loc.y) && close(a.size.h, b.size.h);
            let stacked = close(a.loc.x, b.loc.x) && close(a.size.w, b.size.w);
            let right_of_b = (a.loc.x - (b.loc.x + b.size.w)).abs() <= reach;
            let left_of_b = (b.loc.x - (a.loc.x + a.size.w)).abs() <= reach;
            let below_b = (a.loc.y - (b.loc.y + b.size.h)).abs() <= reach;
            let above_b = (b.loc.y - (a.loc.y + a.size.h)).abs() <= reach;
            let side = if side_by_side && right_of_b {
                Dir::Right
            } else if side_by_side && left_of_b {
                Dir::Left
            } else if stacked && below_b {
                Dir::Down
            } else if stacked && above_b {
                Dir::Up
            } else {
                return None;
            };
            Some((o.id, b, side))
        });
        let Some((other, b, side)) = neighbour else {
            return;
        };
        if let Some(si) = self.split_index_of(other) {
            // The neighbour's split grows by this window on that side.
            let r = self.splits[si].rect;
            let x0 = r.loc.x.min(a.loc.x);
            let y0 = r.loc.y.min(a.loc.y);
            let x1 = (r.loc.x + r.size.w).max(a.loc.x + a.size.w);
            let y1 = (r.loc.y + r.size.h).max(a.loc.y + a.size.h);
            self.splits[si].rect = Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into());
            self.splits[si].insert(other, id, side);
            self.relayout_split(si);
            return;
        }
        let x0 = a.loc.x.min(b.loc.x);
        let y0 = a.loc.y.min(b.loc.y);
        let x1 = (a.loc.x + a.size.w).max(b.loc.x + b.size.w);
        let y1 = (a.loc.y + a.size.h).max(b.loc.y + b.size.h);
        let rect = Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into());
        let mut split = Split::new(self.next_split_id, ws, rect, other);
        self.next_split_id += 1;
        // Keep the sizes the two already have.
        match side {
            Dir::Left | Dir::Right => split.cols[0].weight = b.size.w,
            Dir::Up | Dir::Down => split.cols[0].rows[0].weight = b.size.h,
        }
        split.insert(other, id, side);
        if let Some((c, r)) = split.position(id) {
            match side {
                Dir::Left | Dir::Right => split.cols[c].weight = a.size.w,
                Dir::Up | Dir::Down => split.cols[c].rows[r].weight = a.size.h,
            }
        }
        self.splits.push(split);
        let si = self.splits.len() - 1;
        self.relayout_split(si);
    }

    /// The split handle under the cursor on `output`, if any.
    pub fn split_handle_at_cursor(&self, output: &Output) -> Option<(u64, Handle)> {
        let ws = self.output_ws(output);
        let cam = self.camera(output);
        let p = cam.to_canvas(self.cursor_local(output));
        let gap = self.split_gap();
        let slop = GRAB_PX / cam.zoom;
        self.splits
            .iter()
            .filter(|s| s.ws == ws)
            .find_map(|s| s.handle_at(p, gap, slop).map(|h| (s.id, h)))
    }

    /// The split handle behind an edge of window `idx`: the gap to the
    /// neighbour for an inner edge, the split's own edge for an outer one.
    pub fn split_handle_for_edge(&self, idx: usize, edge: Dir) -> Option<(u64, Handle)> {
        let id = self.windows[idx].id;
        let si = self.split_index_of(id)?;
        let s = &self.splits[si];
        let (c, r) = s.position(id)?;
        let handle = match edge {
            Dir::Left if c > 0 => Handle::Col(c - 1),
            Dir::Right if c + 1 < s.cols.len() => Handle::Col(c),
            Dir::Up if r > 0 => Handle::Row { col: c, i: r - 1 },
            Dir::Down if r + 1 < s.cols[c].rows.len() => Handle::Row { col: c, i: r },
            dir => Handle::Outer(dir),
        };
        Some((s.id, handle))
    }

    /// A press on a split handle; a second one soon after on the same
    /// divider shares it evenly (true: nothing left to drag).
    pub fn press_split_handle(&mut self, split: u64, handle: Handle) -> bool {
        let Some(si) = self.splits.iter().position(|s| s.id == split) else {
            return false;
        };
        let now = Instant::now();
        let double = self.split_press.is_some_and(|p| {
            p.split == split && p.handle == handle && now.duration_since(p.at) < DOUBLE_CLICK
        });
        self.split_press = Some(LastPress {
            split,
            handle,
            at: now,
        });
        if double && matches!(handle, Handle::Col(_) | Handle::Row { .. }) {
            self.splits[si].even_out(handle);
            self.relayout_split(si);
            return true;
        }
        false
    }

    /// The cursor moved while dragging a handle: `base` was the split when
    /// the drag began at canvas point `start`.
    pub fn drag_split_handle(&mut self, base: &Split, handle: Handle, start: Point<f64, Logical>) {
        let Some(si) = self.splits.iter().position(|s| s.id == base.id) else {
            return;
        };
        let Some(output) = self.output_at(self.cursor).or_else(|| self.active_output()) else {
            return;
        };
        let now = self.camera(&output).to_canvas(self.cursor_local(&output));
        let gap = self.split_gap();
        self.splits[si] = Split::dragged(base, handle, now - start, gap);
        self.relayout_split(si);
    }

    /// The split with this id, as it is now (to drag from).
    pub fn split_by_id(&self, id: u64) -> Option<Split> {
        self.splits.iter().find(|s| s.id == id).cloned()
    }
}

/// Cursor for a handle.
pub fn handle_icon(handle: Handle) -> CursorIcon {
    match handle {
        Handle::Col(_) | Handle::Outer(Dir::Left | Dir::Right) => CursorIcon::ColResize,
        Handle::Row { .. } | Handle::Outer(Dir::Up | Dir::Down) => CursorIcon::RowResize,
        Handle::Whole => CursorIcon::Grabbing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn two_columns() -> Split {
        let mut s = Split::new(1, 0, rect(0.0, 0.0, 1010.0, 600.0), 10);
        s.insert(10, 20, Dir::Right);
        s
    }

    #[test]
    fn spans_share_the_length_by_weight() {
        let s = spans(0.0, 1010.0, &[1.0, 1.0], 10.0);
        assert_eq!(s, vec![(0.0, 500.0), (510.0, 500.0)]);
        let s = spans(0.0, 1010.0, &[3.0, 1.0], 10.0);
        assert_eq!(s, vec![(0.0, 750.0), (760.0, 250.0)]);
    }

    #[test]
    fn columns_and_rows_lay_out_side_by_side_and_stacked() {
        let mut s = two_columns();
        s.insert(20, 30, Dir::Down);
        let l = s.layout(10.0);
        assert_eq!(l[0], (10, rect(0.0, 0.0, 500.0, 600.0)));
        assert_eq!(l[1], (20, rect(510.0, 0.0, 500.0, 295.0)));
        assert_eq!(l[2], (30, rect(510.0, 305.0, 500.0, 295.0)));
    }

    #[test]
    fn inserting_left_and_up_goes_before_the_anchor() {
        let mut s = two_columns();
        s.insert(10, 5, Dir::Left);
        assert_eq!(s.members().collect::<Vec<_>>(), vec![5, 10, 20]);
        s.insert(20, 15, Dir::Up);
        assert_eq!(s.position(15), Some((2, 0)));
        assert_eq!(s.position(20), Some((2, 1)));
    }

    #[test]
    fn removing_closes_the_gap_and_drops_empty_columns() {
        let mut s = two_columns();
        s.insert(20, 30, Dir::Down);
        assert!(s.remove(10));
        assert_eq!(s.cols.len(), 1);
        assert_eq!(s.layout(10.0)[0].1, rect(0.0, 0.0, 1010.0, 295.0));
        assert!(!s.remove(99));
    }

    #[test]
    fn the_gap_between_columns_is_a_handle() {
        let s = two_columns();
        assert_eq!(
            s.handle_at((505.0, 300.0).into(), 10.0, 4.0),
            Some(Handle::Col(0))
        );
        assert_eq!(s.handle_at((250.0, 300.0).into(), 10.0, 4.0), None);
        assert_eq!(s.handle_at((2000.0, 300.0).into(), 10.0, 4.0), None);
    }

    #[test]
    fn dragging_a_divider_resizes_both_sides() {
        let s = two_columns();
        let d = Split::dragged(&s, Handle::Col(0), (100.0, 0.0).into(), 10.0);
        let l = d.layout(10.0);
        assert!((l[0].1.size.w - 600.0).abs() < 1e-6);
        assert!((l[1].1.size.w - 400.0).abs() < 1e-6);
        assert!((l[1].1.loc.x - 610.0).abs() < 1e-6);
        // Never below the minimum.
        let d = Split::dragged(&s, Handle::Col(0), (5000.0, 0.0).into(), 10.0);
        assert!((d.layout(10.0)[1].1.size.w - MIN_SPAN).abs() < 1e-6);
    }

    #[test]
    fn an_outer_edge_grows_the_whole_split() {
        let s = two_columns();
        let d = Split::dragged(&s, Handle::Outer(Dir::Right), (200.0, 0.0).into(), 10.0);
        assert_eq!(d.rect, rect(0.0, 0.0, 1210.0, 600.0));
        let l = d.layout(10.0);
        assert!((l[0].1.size.w - 600.0).abs() < 1e-6);
    }

    #[test]
    fn the_whole_split_moves_as_one() {
        let s = two_columns();
        let d = Split::dragged(&s, Handle::Whole, (300.0, -50.0).into(), 10.0);
        assert_eq!(d.rect, rect(300.0, -50.0, 1010.0, 600.0));
        assert_eq!(d.layout(10.0)[1].1.loc, Point::from((810.0, -50.0)));
    }

    #[test]
    fn a_double_click_evens_a_divider_out() {
        let s = two_columns();
        let mut d = Split::dragged(&s, Handle::Col(0), (100.0, 0.0).into(), 10.0);
        d.even_out(Handle::Col(0));
        let l = d.layout(10.0);
        assert!((l[0].1.size.w - l[1].1.size.w).abs() < 1e-6);
    }
}
