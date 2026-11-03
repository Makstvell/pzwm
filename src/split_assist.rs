// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Split assist (`split`, `Super+S`), like Snap Layouts in Windows: the
//! focused window takes the first place of a layout over the view and the
//! other places wait, outlined, while the camera steps back so the other
//! windows show. Click a window (or type its number) and it flies into the
//! next empty place; drag one onto a place; a window that opens now takes
//! one too. `Super+S` or Tab again: the next layout (½ ½, ⅔ ⅓, ½ ¼¼,
//! ⅓ ⅓ ⅓, quarters). Enter or Escape: done, the empty places go and the
//! camera comes back to the split; it is done by itself once all are taken.

use smithay::utils::{Logical, Rectangle};

use crate::{
    canvas::Camera,
    shell::Placement,
    split::{Cell, Column, EMPTY, Split},
    state::State,
};

/// Columns of a layout: (width share, rows).
const LAYOUTS: [&[(f64, usize)]; 5] = [
    &[(1.0, 1), (1.0, 1)],
    &[(2.0, 1), (1.0, 1)],
    &[(1.0, 1), (1.0, 2)],
    &[(1.0, 1), (1.0, 1), (1.0, 1)],
    &[(1.0, 2), (1.0, 2)],
];

pub struct Assist {
    pub split: u64,
    pub layout: usize,
    /// Where the camera goes back to when done.
    pub back: Camera,
    pub ws: usize,
}

/// Layout `layout` filled with `windows` in order, the rest empty places;
/// windows beyond its places get rows of their own in the last column.
pub fn arrange(windows: &[u64], layout: usize) -> Vec<Column> {
    let shape = LAYOUTS[layout % LAYOUTS.len()];
    let mut next = windows.iter().copied();
    let mut cols: Vec<Column> = shape
        .iter()
        .map(|&(weight, rows)| Column {
            weight,
            rows: (0..rows)
                .map(|_| Cell {
                    id: next.next().unwrap_or(EMPTY),
                    weight: 1.0,
                    away: false,
                })
                .collect(),
        })
        .collect();
    if let Some(last) = cols.last_mut() {
        for id in next {
            last.rows.push(Cell {
                id,
                weight: 1.0,
                away: false,
            });
        }
    }
    cols
}

impl State {
    /// `split`: starts the assist on the focused window (making it a split
    /// filling the view first), or, while it runs, the next layout.
    pub fn split_assist(&mut self) -> Result<String, String> {
        if self.assist.is_some() {
            return self.assist_next_layout();
        }
        let idx = self.focused_index().ok_or("no window focused")?;
        let id = self.windows[idx].id;
        let output = self.output_of(idx).ok_or("no output")?;
        let ws = self.output_ws(&output);
        let si = match self.split_index_of(id) {
            Some(si) => si,
            None => {
                if self.windows[idx].placement != Placement::Canvas {
                    self.restore(idx);
                }
                if self.config.canvas.tile_mode == "zoom" {
                    self.camera_to_full_scale(&output);
                }
                let gap = self.config.appearance.gaps as f64;
                let area = self.usable_area(&output).to_f64();
                let screen = Rectangle::new(
                    (area.loc.x + gap, area.loc.y + gap).into(),
                    (area.size.w - 2.0 * gap, area.size.h - 2.0 * gap).into(),
                );
                let rect = self.workspace(ws).target().visible(screen);
                let mut split = Split::new(self.next_split_id, ws, rect, id);
                self.next_split_id += 1;
                split.cols = arrange(&[id], 0);
                self.splits.push(split);
                self.splits.len() - 1
            }
        };
        let split = self.splits[si].id;
        // Room for one more window when every place is taken already.
        if self.splits[si].empty_places() == 0 {
            let members: Vec<u64> = self.splits[si].members().collect();
            let layout = (0..LAYOUTS.len())
                .find(|&l| LAYOUTS[l].iter().map(|c| c.1).sum::<usize>() > members.len())
                .unwrap_or(LAYOUTS.len() - 1);
            self.splits[si].cols = arrange(&members, layout);
        }
        self.relayout_split(si);
        let back = self.workspace(ws).target();
        self.assist = Some(Assist {
            split,
            layout: 0,
            back,
            ws,
        });
        self.assist_show_all(&output, si);
        self.spawn(
            "notify-send -a pzwm -t 6000 -h string:x-canonical-private-synchronous:pzwm-split \
             'Split: fill the empty places' \
             'Click a window (or type its number) · drag one onto a place · open a new one · Super+S or Tab: another layout · Enter: done'",
        );
        Ok("split assist".into())
    }

    /// Steps the camera back so the split and the other windows show.
    fn assist_show_all(&mut self, output: &smithay::output::Output, si: usize) {
        let ws = self.splits[si].ws;
        let mut x0 = self.splits[si].rect.loc.x;
        let mut y0 = self.splits[si].rect.loc.y;
        let mut x1 = x0 + self.splits[si].rect.size.w;
        let mut y1 = y0 + self.splits[si].rect.size.h;
        for m in self.windows.iter().filter(|m| {
            m.ws == ws
                && m.mapped
                && !m.hidden
                && !m.awaiting_pick
                && m.placement == Placement::Canvas
        }) {
            let f = self.canvas_frame(m);
            x0 = x0.min(f.loc.x);
            y0 = y0.min(f.loc.y);
            x1 = x1.max(f.loc.x + f.size.w);
            y1 = y1.max(f.loc.y + f.size.h);
        }
        let bbox = Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into());
        let area = self.usable_area(output).to_f64();
        let now = self.workspace(ws).target();
        let cam = Camera::fitting(bbox, area, 60.0, self.config.canvas.min_zoom, now.zoom);
        self.fly_to(output, cam);
    }

    fn assist_next_layout(&mut self) -> Result<String, String> {
        let Some(a) = self.assist.as_mut() else {
            return Err("no split assist".into());
        };
        a.layout = (a.layout + 1) % LAYOUTS.len();
        let (split, layout) = (a.split, a.layout);
        let si = self
            .splits
            .iter()
            .position(|s| s.id == split)
            .ok_or("the split is gone")?;
        let members: Vec<u64> = self.splits[si].members().collect();
        self.splits[si].cols = arrange(&members, layout);
        self.relayout_split(si);
        Ok("next layout".into())
    }

    /// While the assist runs: `window` goes into the next empty place.
    /// False when it does not run or the window is in that split already.
    pub fn assist_pick(&mut self, window: u64) -> bool {
        let Some(split) = self.assist.as_ref().map(|a| a.split) else {
            return false;
        };
        let Some(si) = self.splits.iter().position(|s| s.id == split) else {
            self.assist = None;
            return false;
        };
        if self.splits[si].position(window).is_some() {
            return false;
        }
        if let Some(idx) = self.index_of_id(window) {
            if self.windows[idx].placement != Placement::Canvas {
                self.restore(idx);
            }
        }
        self.leave_split(window);
        let Some(si) = self.splits.iter().position(|s| s.id == split) else {
            return false;
        };
        if !self.splits[si].fill_place(window) {
            return false;
        }
        self.relayout_split(si);
        self.assist_after_fill(split);
        true
    }

    /// A place of `split` was just taken: done when none is left.
    pub fn assist_after_fill(&mut self, split: u64) {
        let full = self
            .splits
            .iter()
            .find(|s| s.id == split)
            .is_none_or(|s| s.empty_places() == 0);
        if full && self.assist.as_ref().is_some_and(|a| a.split == split) {
            self.assist_done();
        }
    }

    /// Ends the assist: the empty places go, the camera comes back.
    pub fn assist_done(&mut self) {
        let Some(a) = self.assist.take() else {
            return;
        };
        if let Some(si) = self.splits.iter().position(|s| s.id == a.split) {
            self.splits[si].drop_empty_places();
            if self.splits[si].len() <= 1 {
                self.splits.remove(si);
            } else {
                self.relayout_split(si);
            }
        }
        if let Some(output) = self
            .outputs
            .iter()
            .find(|o| self.output_ws(o) == a.ws)
            .cloned()
        {
            self.fly_to(&output, a.back);
        }
        self.queue_redraw_all();
    }

    /// `split pick N`: window number N (as Super+N counts) into the next place.
    pub fn assist_pick_number(&mut self, n: usize) {
        if let Some(&id) = self.numbered_windows().get(n) {
            self.assist_pick(id);
        }
    }

    /// A new window opened while the assist waits for windows: it takes
    /// the next empty place. False when there is none to take.
    pub fn assist_takes_new(&mut self, idx: usize) -> bool {
        let id = self.windows[idx].id;
        let has_place = self.assist.as_ref().is_some_and(|a| {
            self.splits
                .iter()
                .any(|s| s.id == a.split && s.empty_places() > 0)
        });
        if !has_place || !self.assist_pick(id) {
            return false;
        }
        if let Some(idx) = self.index_of_id(id) {
            let window = self.windows[idx].window.clone();
            self.focus_window(Some(window));
        }
        true
    }

    /// Empty places of the splits on `ws` (canvas units), the next one to
    /// be filled marked, for drawing.
    pub fn split_empty_places(
        &self,
        ws: usize,
    ) -> Vec<(u64, usize, Rectangle<f64, Logical>, bool)> {
        let gap = self.config.appearance.gaps as f64;
        let mut out = Vec::new();
        for s in self.splits.iter().filter(|s| s.ws == ws) {
            let mut first = true;
            for (i, (id, r)) in s.layout(gap).into_iter().enumerate() {
                if id == EMPTY {
                    out.push((s.id, i, r, first));
                    first = false;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(cols: &[Column]) -> Vec<Vec<u64>> {
        cols.iter()
            .map(|c| c.rows.iter().map(|r| r.id).collect())
            .collect()
    }

    #[test]
    fn a_layout_is_filled_in_order_with_empty_places_after() {
        assert_eq!(ids(&arrange(&[7], 0)), vec![vec![7], vec![EMPTY]]);
        assert_eq!(ids(&arrange(&[7, 8], 2)), vec![vec![7], vec![8, EMPTY]]);
        assert_eq!(
            ids(&arrange(&[7], 4)),
            vec![vec![7, EMPTY], vec![EMPTY, EMPTY]]
        );
    }

    #[test]
    fn more_windows_than_places_get_rows_of_their_own() {
        assert_eq!(ids(&arrange(&[1, 2, 3], 0)), vec![vec![1], vec![2, 3]]);
    }

    #[test]
    fn the_two_thirds_layout_shares_the_width_two_to_one() {
        let cols = arrange(&[1], 1);
        assert_eq!((cols[0].weight, cols[1].weight), (2.0, 1.0));
    }
}
