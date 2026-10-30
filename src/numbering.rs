// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Window numbers for `window N` and Super+1 … 9, 0: counted by place on
//! the canvas (top-left first, by default) or in the order windows opened,
//! see `[general] window_numbers`. Holding Super and typing more digits
//! goes past 10: Super+1, 2 is window 12.

use std::time::{Duration, Instant};

use smithay::utils::{Logical, Rectangle};

use crate::{shell::Placement, state::State};

/// A further digit counts when it comes this soon after the last one.
const CHORD: Duration = Duration::from_millis(900);
/// Frames whose tops (or left edges) are this close count as one row
/// (column), as a share of the smallest frame.
const BAND_SHARE: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Numbering {
    /// Rows from the top, each left to right.
    Rows,
    /// Rows from the top, each right to left.
    RowsRtl,
    /// Columns from the left, each top to bottom.
    Columns,
    /// The order the windows opened in.
    Opened,
}

impl Numbering {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "rows" | "top-left" => Self::Rows,
            "rows-rtl" | "top-right" => Self::RowsRtl,
            "columns" => Self::Columns,
            "opened" | "launch" => Self::Opened,
            _ => return None,
        })
    }
}

/// Indices of `frames` (with their window ids for `Opened`) in numbering order.
pub fn order(frames: &[(u64, Rectangle<f64, Logical>)], how: Numbering) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..frames.len()).collect();
    if how == Numbering::Opened {
        idx.sort_by_key(|&i| frames[i].0);
        return idx;
    }
    let columns = how == Numbering::Columns;
    // Primary axis: y for rows, x for columns; frames whose start is close
    // to a band's first frame join that band.
    let start = |r: &Rectangle<f64, Logical>| if columns { r.loc.x } else { r.loc.y };
    let along = |r: &Rectangle<f64, Logical>| if columns { r.loc.y } else { r.loc.x };
    let extent = |r: &Rectangle<f64, Logical>| if columns { r.size.w } else { r.size.h };
    let band = frames
        .iter()
        .map(|(_, r)| extent(r))
        .fold(f64::INFINITY, f64::min)
        .max(1.0)
        * BAND_SHARE;
    idx.sort_by(|&a, &b| start(&frames[a].1).total_cmp(&start(&frames[b].1)));
    let mut bands: Vec<Vec<usize>> = Vec::new();
    for i in idx {
        match bands.last_mut() {
            Some(b) if start(&frames[i].1) - start(&frames[b[0]].1) < band => b.push(i),
            _ => bands.push(vec![i]),
        }
    }
    for b in &mut bands {
        b.sort_by(|&a, &c| {
            let (fa, fc) = (along(&frames[a].1), along(&frames[c].1));
            if how == Numbering::RowsRtl {
                fc.total_cmp(&fa)
            } else {
                fa.total_cmp(&fc)
            }
        });
    }
    bands.concat()
}

impl State {
    pub fn numbering(&self) -> Numbering {
        Numbering::parse(&self.config.general.window_numbers).unwrap_or(Numbering::Rows)
    }

    /// Windows of the active workspace (and pinned ones) by number.
    pub fn numbered_windows(&self) -> Vec<u64> {
        let Some(output) = self.active_output() else {
            return Vec::new();
        };
        let ws = self.output_ws(&output);
        let cam = self.camera(&output);
        let frames: Vec<(u64, Rectangle<f64, Logical>)> = self
            .windows
            .iter()
            .filter(|m| self.shown(m) && (m.ws == ws || m.placement == Placement::Pinned))
            .map(|m| {
                // Pinned and maximized windows count where they show now.
                let frame = match self.view(m, &output) {
                    Some(v) if m.placement != Placement::Canvas => {
                        let r = self.frame_rect(m, &v);
                        Rectangle::new(cam.to_canvas(r.loc), r.size.downscale(cam.zoom))
                    }
                    _ => self.canvas_frame(m),
                };
                (m.id, frame)
            })
            .collect();
        order(&frames, self.numbering())
            .into_iter()
            .map(|i| frames[i].0)
            .collect()
    }

    /// Super + a digit: the window with that number, or with the number the
    /// digits typed so far (Super held) make, when there is one.
    pub fn window_digit(&mut self, digit: usize) {
        let now = Instant::now();
        let count = self.numbered_windows().len();
        let longer = self
            .window_chord
            .filter(|&(_, at)| now.duration_since(at) < CHORD)
            .map(|(so_far, _)| so_far * 10 + digit)
            .filter(|&n| (1..=count).contains(&n));
        let number = longer.unwrap_or(if digit == 0 { 10 } else { digit });
        self.window_chord = Some((number, now));
        self.focus_nth(number - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(id: u64, x: f64, y: f64) -> (u64, Rectangle<f64, Logical>) {
        (id, Rectangle::new((x, y).into(), (400.0, 300.0).into()))
    }

    // Three windows on top (one a little lower), two below:
    // 1 . 2 . 3
    // 4 . 5
    fn layout() -> Vec<(u64, Rectangle<f64, Logical>)> {
        vec![
            frame(5, 500.0, 400.0),
            frame(1, 0.0, 0.0),
            frame(3, 1000.0, 30.0),
            frame(2, 500.0, 0.0),
            frame(4, 0.0, 400.0),
        ]
    }

    fn ids(how: Numbering) -> Vec<u64> {
        let f = layout();
        order(&f, how).into_iter().map(|i| f[i].0).collect()
    }

    #[test]
    fn rows_go_top_left_to_bottom_right() {
        assert_eq!(ids(Numbering::Rows), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn rows_rtl_start_at_the_top_right() {
        assert_eq!(ids(Numbering::RowsRtl), vec![3, 2, 1, 5, 4]);
    }

    #[test]
    fn columns_go_down_first() {
        assert_eq!(ids(Numbering::Columns), vec![1, 4, 2, 5, 3]);
    }

    #[test]
    fn opened_follows_the_ids() {
        assert_eq!(ids(Numbering::Opened), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn names_parse() {
        assert_eq!(Numbering::parse("rows"), Some(Numbering::Rows));
        assert_eq!(Numbering::parse("top-right"), Some(Numbering::RowsRtl));
        assert_eq!(Numbering::parse("opened"), Some(Numbering::Opened));
        assert_eq!(Numbering::parse("spiral"), None);
    }
}
