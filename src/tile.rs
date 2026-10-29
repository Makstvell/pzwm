//! Putting a window on part of the screen, beyond plain halves:
//! - `tile` (Super+T): a grid over the screen; drag across the cells (or
//!   move with the arrows, grow with Shift+arrows, Enter) and the focused
//!   window fills them: halves, thirds, quarters, any block of cells;
//! - `snap left|right` pressed again goes ½ → ⅓ → ⅔ of the width;
//! - a window dropped in a screen corner fills that quarter (grabs.rs).

use std::time::{Duration, Instant};

use smithay::{
    input::pointer::CursorIcon,
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::{action::Dir, shell::Placement, state::State};

/// Shares of the width `snap left|right` goes through on repeated presses.
pub const SNAP_SHARES: [f64; 3] = [0.5, 1.0 / 3.0, 2.0 / 3.0];
/// A press of the same snap within this time goes on to the next share.
const SNAP_REPEAT: Duration = Duration::from_secs(3);
/// The largest grid `[canvas] tile_grid` may ask for.
pub const MAX_GRID: u32 = 12;
/// Grid used when `[canvas] tile_grid` is not valid.
const DEFAULT_GRID: (u32, u32) = (6, 4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub col: u32,
    pub row: u32,
}

/// The grid's cells and the selected block, screen units.
pub type TileOverlay = (Vec<Rectangle<f64, Logical>>, Rectangle<f64, Logical>);

/// The grid shown by `tile`.
pub struct TileMode {
    pub output: Output,
    /// The window that gets the cells.
    pub window: u64,
    pub cols: u32,
    pub rows: u32,
    /// Corner where the selection started, and the one that moves.
    pub anchor: Cell,
    pub end: Cell,
}

/// The block of cells between two corners: (first column, first row,
/// last column, last row).
pub fn span(a: Cell, b: Cell) -> (u32, u32, u32, u32) {
    (
        a.col.min(b.col),
        a.row.min(b.row),
        a.col.max(b.col),
        a.row.max(b.row),
    )
}

/// Parses `[canvas] tile_grid`, such as "6x4"; `None` when it is not one.
pub fn parse_grid(s: &str) -> Option<(u32, u32)> {
    let (c, r) = s.trim().split_once(['x', 'X'])?;
    let (c, r) = (c.trim().parse::<u32>().ok()?, r.trim().parse::<u32>().ok()?);
    ((1..=MAX_GRID).contains(&c) && (1..=MAX_GRID).contains(&r)).then_some((c, r))
}

/// Screen rectangle of the cells from `(c0, r0)` to `(c1, r1)` (inclusive)
/// of a `cols` × `rows` grid over `area`, with `gap` around every cell.
pub fn cells_rect(
    area: Rectangle<f64, Logical>,
    gap: f64,
    (cols, rows): (u32, u32),
    (c0, r0, c1, r1): (u32, u32, u32, u32),
) -> Rectangle<f64, Logical> {
    let cell_w = (area.size.w - gap * (cols as f64 + 1.0)) / cols as f64;
    let cell_h = (area.size.h - gap * (rows as f64 + 1.0)) / rows as f64;
    let (cw, ch) = ((c1 - c0 + 1) as f64, (r1 - r0 + 1) as f64);
    Rectangle::new(
        (
            area.loc.x + gap + c0 as f64 * (cell_w + gap),
            area.loc.y + gap + r0 as f64 * (cell_h + gap),
        )
            .into(),
        (
            cw * cell_w + (cw - 1.0) * gap,
            ch * cell_h + (ch - 1.0) * gap,
        )
            .into(),
    )
}

/// The grid cell under a screen point, clamped to the grid.
pub fn cell_at(
    area: Rectangle<f64, Logical>,
    (cols, rows): (u32, u32),
    p: Point<f64, Logical>,
) -> Cell {
    let fx = ((p.x - area.loc.x) / area.size.w).clamp(0.0, 0.999_999);
    let fy = ((p.y - area.loc.y) / area.size.h).clamp(0.0, 0.999_999);
    Cell {
        col: (fx * cols as f64) as u32,
        row: (fy * rows as f64) as u32,
    }
}

/// A full-height strip of `share` of the width at the left or right of `area`.
pub fn share_rect(
    area: Rectangle<f64, Logical>,
    gap: f64,
    dir: Dir,
    share: f64,
) -> Rectangle<f64, Logical> {
    let w = (area.size.w - 3.0 * gap) * share;
    let x = match dir {
        Dir::Left => area.loc.x + gap,
        _ => area.loc.x + area.size.w - gap - w,
    };
    Rectangle::new(
        (x, area.loc.y + gap).into(),
        (w, area.size.h - 2.0 * gap).into(),
    )
}

/// A quarter of `area`: left or right, top or bottom.
pub fn quarter_rect(
    area: Rectangle<f64, Logical>,
    gap: f64,
    left: bool,
    top: bool,
) -> Rectangle<f64, Logical> {
    let (c, r) = (u32::from(!left), u32::from(!top));
    cells_rect(area, gap, (2, 2), (c, r, c, r))
}

impl State {
    pub fn tile_grid(&self) -> (u32, u32) {
        parse_grid(&self.config.canvas.tile_grid).unwrap_or(DEFAULT_GRID)
    }

    /// `tile` opens the grid (again: closes it); while it is open
    /// `tile left|right|up|down`, `tile grow DIR`, `tile apply`, `tile cancel`.
    pub fn tile_command(&mut self, arg: &str) -> Result<String, String> {
        let arg = arg.trim();
        let (verb, rest) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
        let dir = |s: &str| Dir::parse(s.trim()).ok_or_else(|| format!("unknown direction {s}"));
        match verb {
            "" if self.tile.is_none() => {}
            "" | "cancel" => {
                self.end_tile();
                return Ok("tile cancelled".into());
            }
            "apply" => {
                self.tile_apply();
                return Ok("tiled".into());
            }
            "grow" => {
                self.tile_step(dir(rest)?, true);
                return Ok("ok".into());
            }
            other => {
                self.tile_step(dir(other)?, false);
                return Ok("ok".into());
            }
        }
        let idx = self.focused_index().ok_or("no focused window")?;
        let output = self
            .output_of(idx)
            .or_else(|| self.active_output())
            .ok_or("no output")?;
        let grid = self.tile_grid();
        let area = self.usable_area(&output).to_f64();
        let start = cell_at(area, grid, self.cursor_local(&output));
        self.tile = Some(TileMode {
            output,
            window: self.windows[idx].id,
            cols: grid.0,
            rows: grid.1,
            anchor: start,
            end: start,
        });
        self.cursor_override = Some(CursorIcon::Crosshair);
        self.queue_redraw_all();
        Ok("drag across the cells, or arrows and Enter".into())
    }

    pub fn end_tile(&mut self) {
        if self.tile.take().is_some() && self.interaction.is_none() {
            self.cursor_override = None;
        }
        self.queue_redraw_all();
    }

    /// Moves the selection one cell; `grow` moves only its free corner.
    pub fn tile_step(&mut self, dir: Dir, grow: bool) {
        let Some(t) = self.tile.as_mut() else {
            return;
        };
        let (cols, rows) = (t.cols, t.rows);
        let step = |c: Cell| match dir {
            Dir::Left => Cell {
                col: c.col.saturating_sub(1),
                ..c
            },
            Dir::Right => Cell {
                col: (c.col + 1).min(cols - 1),
                ..c
            },
            Dir::Up => Cell {
                row: c.row.saturating_sub(1),
                ..c
            },
            Dir::Down => Cell {
                row: (c.row + 1).min(rows - 1),
                ..c
            },
        };
        let end = step(t.end);
        if !grow {
            let anchor = step(t.anchor);
            // The whole block moves only while all of it stays on the grid.
            if end == t.end || anchor == t.anchor {
                return;
            }
            t.anchor = anchor;
        }
        t.end = end;
        self.queue_redraw_all();
    }

    /// The pointer picks cells: `start` begins a new block under it.
    pub fn tile_drag_to_cursor(&mut self, start: bool) {
        let Some(output) = self.tile.as_ref().map(|t| t.output.clone()) else {
            return;
        };
        let area = self.usable_area(&output).to_f64();
        let cell = cell_at(area, self.tile_grid(), self.cursor_local(&output));
        if let Some(t) = self.tile.as_mut() {
            if start {
                t.anchor = cell;
            }
            t.end = cell;
        }
        self.queue_redraw(&output);
    }

    /// The window takes the selected cells; the grid closes.
    pub fn tile_apply(&mut self) {
        let Some(t) = self.tile.take() else {
            return;
        };
        if self.interaction.is_none() {
            self.cursor_override = None;
        }
        let area = self.usable_area(&t.output).to_f64();
        let gap = self.config.appearance.gaps as f64;
        let rect = cells_rect(area, gap, (t.cols, t.rows), span(t.anchor, t.end));
        if let Some(idx) = self.index_of_id(t.window) {
            if self.windows[idx].placement != Placement::Canvas {
                self.restore(idx);
            }
        }
        if let Some(idx) = self.index_of_id(t.window) {
            self.place_in_screen_rect(idx, &t.output, rect);
        }
        self.queue_redraw_all();
    }

    /// The grid's cells and the selected block on `output`, screen units.
    pub fn tile_overlay(&self, output: &Output) -> Option<TileOverlay> {
        let t = self.tile.as_ref().filter(|t| &t.output == output)?;
        let area = self.usable_area(output).to_f64();
        let gap = self.config.appearance.gaps as f64;
        let grid = (t.cols, t.rows);
        let cells = (0..t.rows)
            .flat_map(|r| (0..t.cols).map(move |c| (c, r, c, r)))
            .map(|cell| cells_rect(area, gap, grid, cell))
            .collect();
        Some((cells, cells_rect(area, gap, grid, span(t.anchor, t.end))))
    }

    /// `snap left|right`: the strip for this press, going on to the next
    /// share when the same window was snapped the same way just before.
    pub fn snap_share_rect(
        &mut self,
        idx: usize,
        output: &Output,
        dir: Dir,
    ) -> Rectangle<f64, Logical> {
        let id = self.windows[idx].id;
        let now = Instant::now();
        let step = match self.last_snap {
            Some((last_id, last_dir, step, at))
                if last_id == id && last_dir == dir && now.duration_since(at) < SNAP_REPEAT =>
            {
                (step + 1) % SNAP_SHARES.len()
            }
            _ => 0,
        };
        self.last_snap = Some((id, dir, step, now));
        let area = self.usable_area(output).to_f64();
        share_rect(
            area,
            self.config.appearance.gaps as f64,
            dir,
            SNAP_SHARES[step],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn grid_sizes_parse() {
        assert_eq!(parse_grid("6x4"), Some((6, 4)));
        assert_eq!(parse_grid(" 3 X 2 "), Some((3, 2)));
        assert_eq!(parse_grid("0x4"), None);
        assert_eq!(parse_grid("13x4"), None);
        assert_eq!(parse_grid("six"), None);
    }

    #[test]
    fn one_cell_and_a_block_of_cells() {
        let area = rect(0.0, 0.0, 1000.0, 500.0);
        assert_eq!(
            cells_rect(area, 0.0, (2, 2), (1, 1, 1, 1)),
            rect(500.0, 250.0, 500.0, 250.0)
        );
        // Both columns of the top row: the top half.
        assert_eq!(
            cells_rect(area, 0.0, (2, 2), (0, 0, 1, 0)),
            rect(0.0, 0.0, 1000.0, 250.0)
        );
        // Gaps around every cell, also at the edges.
        assert_eq!(
            cells_rect(area, 10.0, (2, 1), (0, 0, 0, 0)),
            rect(10.0, 10.0, 485.0, 480.0)
        );
    }

    #[test]
    fn cell_under_a_point_is_clamped() {
        let area = rect(0.0, 0.0, 600.0, 400.0);
        assert_eq!(
            cell_at(area, (6, 4), (150.0, 50.0).into()),
            Cell { col: 1, row: 0 }
        );
        assert_eq!(
            cell_at(area, (6, 4), (900.0, 900.0).into()),
            Cell { col: 5, row: 3 }
        );
        assert_eq!(
            cell_at(area, (6, 4), (-5.0, -5.0).into()),
            Cell { col: 0, row: 0 }
        );
    }

    #[test]
    fn strips_and_quarters() {
        let area = rect(0.0, 0.0, 900.0, 600.0);
        assert_eq!(
            share_rect(area, 0.0, Dir::Right, 1.0 / 3.0),
            rect(600.0, 0.0, 300.0, 600.0)
        );
        assert_eq!(
            share_rect(area, 0.0, Dir::Left, 0.5),
            rect(0.0, 0.0, 450.0, 600.0)
        );
        assert_eq!(
            quarter_rect(area, 0.0, false, false),
            rect(450.0, 300.0, 450.0, 300.0)
        );
    }

    #[test]
    fn span_orders_the_corners() {
        let (a, b) = (Cell { col: 3, row: 2 }, Cell { col: 1, row: 0 });
        assert_eq!(span(a, b), (1, 0, 3, 2));
    }
}
