//! Drawing on the canvas: freehand notes and arrows between windows, like
//! on a whiteboard. Strokes live in canvas coordinates, so they pan and
//! zoom with the windows, and are kept per workspace in
//! `$XDG_STATE_HOME/pzwm/drawings.json` (and in desktop profiles).
//!
//! `draw` turns the pen on: left drag draws, Shift + left drag draws an
//! arrow, right drag erases whole strokes. While it is on, 1 … 9 pick a
//! color, Ctrl+Z undoes, Delete clears the workspace and Escape stops.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use smithay::{
    output::Output,
    utils::{Logical, Point},
};
use tracing::warn;

use crate::{canvas::WORKSPACES, grabs, profiles::state_dir, state::State};

/// A new point is recorded once the pointer moved this far (screen px).
const MIN_STEP_PX: f64 = 2.0;
/// Finished strokes are simplified to within this distance (screen px).
const SIMPLIFY_PX: f64 = 0.6;
/// The eraser removes strokes passing this close to it (screen px).
const ERASER_PX: f64 = 10.0;
/// Arrow head: length in pen widths and half-angle.
const HEAD_WIDTHS: f64 = 4.5;
const HEAD_ANGLE: f64 = 0.45;
/// Strokes kept per workspace; the oldest go first.
const MAX_STROKES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    #[serde(skip)]
    pub id: u64,
    pub color: [f32; 4],
    /// Pen width, canvas units.
    pub width: f64,
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub arrow: bool,
}

impl Stroke {
    /// The line segments to draw, arrow head included.
    pub fn segments(&self) -> Vec<([f64; 2], [f64; 2])> {
        let mut segs: Vec<([f64; 2], [f64; 2])> =
            self.points.windows(2).map(|w| (w[0], w[1])).collect();
        if self.points.len() == 1 {
            segs.push((self.points[0], self.points[0]));
        }
        if self.arrow {
            if let [.., a, b] = self.points[..] {
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len > 0.0 {
                    let head = (self.width * HEAD_WIDTHS).min(len * 0.6);
                    let back = dy.atan2(dx) + std::f64::consts::PI;
                    for side in [-HEAD_ANGLE, HEAD_ANGLE] {
                        let t = back + side;
                        segs.push((b, [b[0] + head * t.cos(), b[1] + head * t.sin()]));
                    }
                }
            }
        }
        segs
    }

    fn near(&self, p: [f64; 2], reach: f64) -> bool {
        let reach = reach + self.width / 2.0;
        self.segments()
            .iter()
            .any(|&(a, b)| segment_distance(p, a, b) <= reach)
    }
}

fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (bx, by) = (b[0] - a[0], b[1] - a[1]);
    let (px, py) = (p[0] - a[0], p[1] - a[1]);
    let len2 = bx * bx + by * by;
    let t = if len2 > 0.0 {
        ((px * bx + py * by) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (px - bx * t).hypot(py - by * t)
}

/// Ramer–Douglas–Peucker: drops points that lie within `tolerance` of the
/// line through their neighbours, so long strokes stay cheap to draw.
pub fn simplify(points: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    let (far, dist) = points[1..points.len() - 1]
        .iter()
        .enumerate()
        .map(|(i, &p)| (i + 1, segment_distance(p, first, last)))
        .fold(
            (0, 0.0),
            |best, cur| if cur.1 > best.1 { cur } else { best },
        );
    if dist <= tolerance {
        return vec![first, last];
    }
    let mut left = simplify(&points[..=far], tolerance);
    left.pop();
    left.extend(simplify(&points[far..], tolerance));
    left
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedStroke {
    pub ws: usize,
    #[serde(flatten)]
    pub stroke: Stroke,
}

#[derive(Debug, Default)]
pub struct Drawing {
    pub strokes: [Vec<Stroke>; WORKSPACES],
    /// The pen is on: mouse buttons draw instead of reaching windows.
    pub active: bool,
    pub hidden: bool,
    pub color: usize,
    next_id: u64,
}

impl Drawing {
    fn path() -> PathBuf {
        state_dir().join("drawings.json")
    }

    pub fn load() -> Self {
        let mut drawing = Self::default();
        let Ok(text) = std::fs::read_to_string(Self::path()) else {
            return drawing;
        };
        match serde_json::from_str::<Vec<SavedStroke>>(&text) {
            Ok(saved) => drawing.replace_all(saved),
            Err(err) => warn!("{}: {err}", Self::path().display()),
        }
        drawing
    }

    /// Every stroke with its workspace, for saving.
    pub fn saved(&self) -> Vec<SavedStroke> {
        (0..WORKSPACES)
            .flat_map(|ws| {
                self.strokes[ws].iter().map(move |s| SavedStroke {
                    ws,
                    stroke: s.clone(),
                })
            })
            .collect()
    }

    /// Replaces every drawing (loading a file or a profile).
    pub fn replace_all(&mut self, saved: Vec<SavedStroke>) {
        self.strokes = Default::default();
        for SavedStroke { ws, mut stroke } in saved {
            let finite = stroke.points.iter().flatten().all(|v| v.is_finite());
            if ws < WORKSPACES && finite && !stroke.points.is_empty() && stroke.width > 0.0 {
                stroke.id = self.new_id();
                self.strokes[ws].push(stroke);
            }
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        let result = serde_json::to_string(&self.saved())
            .map_err(|e| e.to_string())
            .and_then(|text| {
                std::fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
                std::fs::write(&path, text).map_err(|e| e.to_string())
            });
        if let Err(err) = result {
            warn!("{}: {err}", path.display());
        }
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn stroke_mut(&mut self, ws: usize, id: u64) -> Option<&mut Stroke> {
        self.strokes[ws].iter_mut().find(|s| s.id == id)
    }
}

impl State {
    /// `draw [on|off|undo|clear|hide|color N|color next]`; plain `draw`
    /// turns the pen on or off.
    pub fn draw_command(&mut self, args: &[&str]) -> Result<String, String> {
        let ws = self
            .active_output()
            .map(|o| self.output_ws(&o))
            .ok_or("no output")?;
        match args {
            [] => self.set_pen(!self.draw.active),
            ["on"] => self.set_pen(true),
            ["off"] => self.set_pen(false),
            ["undo"] => {
                self.draw.strokes[ws].pop().ok_or("nothing to undo")?;
                self.draw.save();
            }
            ["clear"] => {
                self.draw.strokes[ws].clear();
                self.draw.save();
            }
            ["hide"] => self.draw.hidden = !self.draw.hidden,
            ["color", "next"] => {
                let n = self.config.draw.colors.len().max(1);
                self.draw.color = (self.draw.color + 1) % n;
            }
            ["color", n] => {
                let n: usize = n.parse().map_err(|_| format!("bad color number `{n}`"))?;
                if n == 0 || n > self.config.draw.colors.len() {
                    return Err(format!(
                        "color {n}: [draw] colors has {}",
                        self.config.draw.colors.len()
                    ));
                }
                self.draw.color = n - 1;
            }
            _ => {
                return Err(
                    "usage: draw [on | off | undo | clear | hide | color N | color next]".into(),
                );
            }
        }
        self.queue_redraw_all();
        Ok("ok".into())
    }

    fn set_pen(&mut self, on: bool) {
        self.draw.active = on;
        if on {
            self.draw.hidden = false;
        }
    }

    fn pen_color(&self) -> [f32; 4] {
        let colors = &self.config.draw.colors;
        colors
            .get(self.draw.color)
            .or(colors.first())
            .copied()
            .unwrap_or([1.0, 0.3, 0.4, 1.0])
    }

    /// Starts a stroke (left) or the eraser (right) at the cursor. Returns
    /// false when the button does nothing with the pen.
    pub fn start_drawing(
        &mut self,
        button: u32,
        arrow: bool,
        serial: smithay::utils::Serial,
    ) -> bool {
        let Some(output) = self.active_output() else {
            return false;
        };
        let ws = self.output_ws(&output);
        let erase = match button {
            crate::input::BTN_LEFT => false,
            crate::input::BTN_RIGHT => true,
            _ => return false,
        };
        let id = if erase {
            0
        } else {
            let cam = self.camera(&output);
            let p = cam.to_canvas(self.cursor_local(&output));
            let stroke = Stroke {
                id: self.draw.new_id(),
                color: self.pen_color(),
                width: self.config.draw.width.max(0.5) / cam.zoom,
                points: vec![[p.x, p.y]],
                arrow,
            };
            let id = stroke.id;
            let strokes = &mut self.draw.strokes[ws];
            if strokes.len() >= MAX_STROKES {
                strokes.remove(0);
            }
            strokes.push(stroke);
            id
        };
        grabs::start_draw(self, output, ws, id, erase, serial, button);
        if erase {
            self.erase_at_cursor(ws);
        }
        true
    }

    /// Extends the stroke being drawn to the cursor.
    pub fn draw_motion(&mut self, output: &Output, ws: usize, id: u64) {
        let cam = self.camera(output);
        let p = cam.to_canvas(self.cursor_local(output));
        let min_step = MIN_STEP_PX / cam.zoom;
        let Some(stroke) = self.draw.stroke_mut(ws, id) else {
            return;
        };
        let p = [p.x, p.y];
        if stroke.arrow {
            stroke.points.truncate(1);
            stroke.points.push(p);
        } else if let Some(last) = stroke.points.last() {
            if (p[0] - last[0]).hypot(p[1] - last[1]) >= min_step {
                stroke.points.push(p);
            }
        }
        self.queue_redraw(output);
    }

    pub fn draw_finished(&mut self, output: &Output, ws: usize, id: u64) {
        let tolerance = SIMPLIFY_PX / self.camera(output).zoom;
        if let Some(stroke) = self.draw.stroke_mut(ws, id) {
            if !stroke.arrow {
                stroke.points = simplify(&stroke.points, tolerance);
            }
        }
        self.draw.save();
    }

    /// Removes the strokes under the cursor.
    pub fn erase_at_cursor(&mut self, ws: usize) {
        let Some(output) = self.active_output() else {
            return;
        };
        let cam = self.camera(&output);
        let p: Point<f64, Logical> = cam.to_canvas(self.cursor_local(&output));
        let reach = ERASER_PX / cam.zoom;
        let before = self.draw.strokes[ws].len();
        self.draw.strokes[ws].retain(|s| !s.near([p.x, p.y], reach));
        if self.draw.strokes[ws].len() != before {
            self.queue_redraw(&output);
        }
    }

    pub fn erase_finished(&mut self) {
        self.draw.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(points: Vec<[f64; 2]>, arrow: bool) -> Stroke {
        Stroke {
            id: 1,
            color: [1.0; 4],
            width: 2.0,
            points,
            arrow,
        }
    }

    #[test]
    fn simplify_drops_points_on_a_straight_line() {
        let line: Vec<[f64; 2]> = (0..50).map(|i| [i as f64, 0.01 * (i % 2) as f64]).collect();
        assert_eq!(simplify(&line, 0.5), vec![[0.0, 0.0], [49.0, 0.01]]);
    }

    #[test]
    fn simplify_keeps_corners() {
        let corner = [
            [0.0, 0.0],
            [5.0, 0.0],
            [10.0, 0.0],
            [10.0, 5.0],
            [10.0, 10.0],
        ];
        assert_eq!(
            simplify(&corner, 0.5),
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]
        );
    }

    #[test]
    fn an_arrow_gets_two_head_segments() {
        let s = stroke(vec![[0.0, 0.0], [100.0, 0.0]], true);
        let segs = s.segments();
        assert_eq!(segs.len(), 3);
        // Both head lines start at the tip and point back.
        assert!(
            segs[1..]
                .iter()
                .all(|(a, b)| *a == [100.0, 0.0] && b[0] < 100.0)
        );
    }

    #[test]
    fn a_dot_is_one_zero_length_segment() {
        assert_eq!(stroke(vec![[3.0, 4.0]], false).segments().len(), 1);
    }

    #[test]
    fn eraser_hits_strokes_near_the_line_only() {
        let s = stroke(vec![[0.0, 0.0], [100.0, 0.0]], false);
        assert!(s.near([50.0, 5.0], 5.0));
        assert!(!s.near([50.0, 20.0], 5.0));
    }

    #[test]
    fn bad_saved_strokes_are_dropped() {
        let mut d = Drawing::default();
        let good = SavedStroke {
            ws: 1,
            stroke: stroke(vec![[0.0, 0.0]], false),
        };
        let off_range = SavedStroke {
            ws: 99,
            stroke: stroke(vec![[0.0, 0.0]], false),
        };
        let nan = SavedStroke {
            ws: 0,
            stroke: stroke(vec![[f64::NAN, 0.0]], false),
        };
        d.replace_all(vec![good, off_range, nan]);
        assert_eq!(d.strokes[1].len(), 1);
        assert_eq!(d.saved().len(), 1);
    }

    #[test]
    fn strokes_round_trip_through_json() {
        let saved = vec![SavedStroke {
            ws: 2,
            stroke: stroke(vec![[1.0, 2.0], [3.0, 4.0]], true),
        }];
        let text = serde_json::to_string(&saved).unwrap();
        let back: Vec<SavedStroke> = serde_json::from_str(&text).unwrap();
        assert_eq!(back[0].ws, 2);
        assert_eq!(back[0].stroke.points, saved[0].stroke.points);
        assert!(back[0].stroke.arrow);
    }
}
