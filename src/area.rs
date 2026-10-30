// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Where new windows go besides the middle of the view:
//! - an **area** the user drags out on the canvas (`area`, Super+O, or
//!   Super+Ctrl + left drag): the next window that opens fills it;
//! - with `[canvas] new_window_placement = "pick"` every new window waits,
//!   hidden, until its area is dragged out and then takes exactly that
//!   area (Escape: the middle of the view; Enter: beside the focused
//!   window, see below);
//! - **beside the focused window** at the ideal size (Enter while picking,
//!   or always with `new_window_placement = "beside"`): the next free
//!   place in the reading order of `[general] window_numbers`, to the right
//!   for rows, below for columns, to the left for rows-rtl;
//! - next to a **sibling**: a window an app opens by itself, while it
//!   already has one on the workspace (same process, nothing launched by
//!   the user just before), opens beside it (small ones over it) and the
//!   camera fits it, the way KDE keeps an app's windows together. A window
//!   the user launches is never a sibling, even of a running app.

use std::time::{Duration, Instant};

use smithay::reexports::wayland_server::Resource;
use smithay::{
    input::pointer::CursorIcon,
    output::Output,
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Logical, Point, Rectangle, Size},
};

use crate::{canvas::Camera, numbering::Numbering, shell::Placement, state::State};

/// How long a picked area waits for its window.
pub const AREA_TIMEOUT: Duration = Duration::from_secs(60);
/// A drag smaller than this (screen pixels) cancels the pick.
const MIN_AREA_PX: f64 = 40.0;
/// A window at most this share of its sibling both ways counts as a dialog
/// and is centered over it instead of put beside it.
const DIALOG_SHARE: f64 = 0.6;
/// Space between a window and its sibling, canvas units, at least.
const MIN_SIBLING_GAP: f64 = 16.0;
/// A window appearing this soon after a launch (a `spawn` bind, the
/// launcher) counts as one the user opened.
const LAUNCH_WINDOW: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct AreaState {
    /// Waiting for the drag that picks the area.
    pub selecting: bool,
    /// Command started once the area is picked (`area spawn …`).
    pub spawn: Option<String>,
    /// The picked area, waiting for the next window.
    pub pending: Option<PendingArea>,
    /// The window the current pick places ("pick" mode).
    pub window: Option<u64>,
    /// Windows still waiting for their place, oldest first.
    pub waiting: Vec<u64>,
    /// When the user launched something whose window has not come yet.
    pub launches: Vec<Instant>,
}

#[derive(Clone, Copy, Debug)]
pub struct PendingArea {
    pub ws: usize,
    /// Canvas rectangle of the whole frame, title bar included.
    pub rect: Rectangle<f64, Logical>,
    pub until: Instant,
}

/// Rectangle spanned by two corners, in any order.
pub fn span(a: Point<f64, Logical>, b: Point<f64, Logical>) -> Rectangle<f64, Logical> {
    Rectangle::new(
        (a.x.min(b.x), a.y.min(b.y)).into(),
        ((a.x - b.x).abs(), (a.y - b.y).abs()).into(),
    )
}

/// Top-left of a frame of `size` beside `sibling`: right, below, left or
/// above it, the first place that overlaps none of `others`; right when
/// every side is taken.
pub fn beside(
    sibling: Rectangle<f64, Logical>,
    size: Size<f64, Logical>,
    others: &[Rectangle<f64, Logical>],
    gap: f64,
) -> Point<f64, Logical> {
    let s = sibling;
    let candidates: [Point<f64, Logical>; 4] = [
        (s.loc.x + s.size.w + gap, s.loc.y).into(),
        (s.loc.x, s.loc.y + s.size.h + gap).into(),
        (s.loc.x - gap - size.w, s.loc.y).into(),
        (s.loc.x, s.loc.y - gap - size.h).into(),
    ];
    candidates
        .into_iter()
        .find(|&p| {
            let r = Rectangle::new(p, size);
            !others.iter().any(|o| o.overlaps(r))
        })
        .unwrap_or(candidates[0])
}

/// Most windows stepped over looking for a free place in reading order.
const MAX_STEPS: usize = 64;

/// Top-left of a frame of `size` next to `anchor` in reading order:
/// right of it for rows (left for rows-rtl, below for columns), past any
/// of `others` in the way.
pub fn next_in_order(
    anchor: Rectangle<f64, Logical>,
    size: Size<f64, Logical>,
    others: &[Rectangle<f64, Logical>],
    gap: f64,
    how: Numbering,
) -> Point<f64, Logical> {
    let a = anchor;
    let mut p: Point<f64, Logical> = match how {
        Numbering::Rows | Numbering::Opened => (a.loc.x + a.size.w + gap, a.loc.y).into(),
        Numbering::RowsRtl => (a.loc.x - gap - size.w, a.loc.y).into(),
        Numbering::Columns => (a.loc.x, a.loc.y + a.size.h + gap).into(),
    };
    for _ in 0..MAX_STEPS {
        let r = Rectangle::new(p, size);
        let Some(o) = others.iter().find(|o| o.overlaps(r)) else {
            break;
        };
        // Step past the window in the way, keeping the row (column).
        match how {
            Numbering::Rows | Numbering::Opened => p.x = o.loc.x + o.size.w + gap,
            Numbering::RowsRtl => p.x = o.loc.x - gap - size.w,
            Numbering::Columns => p.y = o.loc.y + o.size.h + gap,
        }
    }
    p
}

/// Parses `X,Y WxH` (canvas units) for `area rect`.
fn parse_rect(arg: &str) -> Option<Rectangle<f64, Logical>> {
    let (pos, size) = arg.trim().split_once(char::is_whitespace)?;
    let (x, y) = pos.split_once(',')?;
    let (w, h) = size.trim().split_once('x')?;
    let num = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    let (x, y, w, h) = (num(x)?, num(y)?, num(w)?, num(h)?);
    (w > 0.0 && h > 0.0).then(|| Rectangle::new((x, y).into(), (w, h).into()))
}

impl State {
    /// `area [spawn CMD] | cancel | rect X,Y WxH`.
    pub fn area_command(&mut self, args: &str) -> Result<String, String> {
        let args = args.trim();
        let (verb, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
        match verb {
            "" | "pick" => self.begin_area_pick(None),
            "beside" => self.area_beside(),
            "spawn" if !rest.trim().is_empty() => self.begin_area_pick(Some(rest.trim().into())),
            "cancel" => {
                self.cancel_area();
                Ok("area cancelled".into())
            }
            "rect" => {
                let rect = parse_rect(rest).ok_or("usage: area rect X,Y WxH")?;
                let output = self.active_output().ok_or("no output")?;
                let ws = self.output_ws(&output);
                // A window is waiting for its place: this is it.
                if let Some(id) = self.area.window.take() {
                    self.area.selecting = false;
                    if self.interaction.is_none() {
                        self.cursor_override = None;
                    }
                    self.show_picked(id, Some((ws, rect)));
                    self.pick_next_window();
                    return Ok("placed".into());
                }
                self.set_pending_area(ws, rect);
                Ok("the next window opens in the area".into())
            }
            _ => Err("usage: area [spawn CMD] | beside | cancel | rect X,Y WxH".into()),
        }
    }

    fn begin_area_pick(&mut self, spawn: Option<String>) -> Result<String, String> {
        self.area.selecting = true;
        self.area.spawn = spawn;
        self.cursor_override = Some(CursorIcon::Crosshair);
        self.queue_redraw_all();
        Ok("drag an area for the next window (Enter: beside the focused one, Esc cancels)".into())
    }

    /// Escape / `area cancel`: drops the pick and a waiting area; a window
    /// whose place was being picked opens where it would have without.
    pub fn cancel_area(&mut self) {
        if self.area.selecting && self.interaction.is_none() {
            self.cursor_override = None;
        }
        self.area.selecting = false;
        self.area.spawn = None;
        self.area.pending = None;
        if let Some(id) = self.area.window.take() {
            self.show_picked(id, None);
        }
        self.queue_redraw_all();
        self.pick_next_window();
    }

    /// A `spawn` from a bind, the launcher or `pzwm msg`: its window,
    /// whatever app it is, is the user's own, not an app's further window.
    pub fn note_user_launch(&mut self) {
        let now = Instant::now();
        self.area
            .launches
            .retain(|&t| now.duration_since(t) < LAUNCH_WINDOW);
        self.area.launches.push(now);
    }

    /// Uses up the oldest recent launch for a window that just appeared.
    pub fn take_user_launch(&mut self) -> bool {
        let now = Instant::now();
        self.area
            .launches
            .retain(|&t| now.duration_since(t) < LAUNCH_WINDOW);
        if self.area.launches.is_empty() {
            return false;
        }
        self.area.launches.remove(0);
        true
    }

    /// Process id of the client a window belongs to.
    fn client_pid(&self, m: &crate::shell::Managed) -> Option<i32> {
        let surface = m.wl_surface()?;
        let client = surface.client()?;
        client
            .get_credentials(&self.display_handle)
            .ok()
            .map(|c| c.pid)
    }

    /// "pick" mode: a new window waits for its place instead of opening.
    pub fn wait_for_pick(&mut self, idx: usize) {
        self.windows[idx].awaiting_pick = true;
        self.area.waiting.push(self.windows[idx].id);
        self.pick_next_window();
    }

    /// Starts picking the place of the oldest waiting window, if no pick
    /// is going on.
    pub fn pick_next_window(&mut self) {
        if self.area.selecting {
            return;
        }
        while !self.area.waiting.is_empty() {
            let id = self.area.waiting.remove(0);
            let Some(idx) = self
                .index_of_id(id)
                .filter(|&i| self.windows[i].awaiting_pick)
            else {
                continue;
            };
            let app = self.app_id(&self.windows[idx]).unwrap_or_default();
            self.area.window = Some(id);
            self.area.selecting = true;
            self.cursor_override = Some(CursorIcon::Crosshair);
            let quote = crate::bindings::shell_quote;
            self.spawn(&format!(
                "notify-send -a pzwm -t 4000 \
                 -h string:x-canonical-private-synchronous:pzwm-pick {} {}",
                quote(&format!("Place {app}")),
                quote("Drag out its area · Enter: beside the focused window · Esc: the middle"),
            ));
            self.queue_redraw_all();
            return;
        }
    }

    /// Shows a window that waited for its place: in `rect` (canvas, whole
    /// frame), or where map_window put it when `None`.
    fn show_picked(&mut self, id: u64, rect: Option<(usize, Rectangle<f64, Logical>)>) {
        let Some(idx) = self.index_of_id(id) else {
            return;
        };
        let title = self.title_height(&self.windows[idx]);
        let m = &mut self.windows[idx];
        m.awaiting_pick = false;
        m.opened_at = Some(Instant::now());
        if let Some((ws, rect)) = rect {
            m.ws = ws;
            m.pos = Point::from((rect.loc.x, rect.loc.y + title));
            let size = Size::from((
                rect.size.w.round() as i32,
                (rect.size.h - title).round().max(1.0) as i32,
            ));
            if (m.size().w - size.w as f64).abs() > 1.0 || (m.size().h - size.h as f64).abs() > 1.0
            {
                self.configure(idx, Some(size), false, false);
            }
        }
        let window = self.windows[idx].window.clone();
        self.focus_window(Some(window));
        self.queue_redraw_all();
    }

    /// Enter while picking (`area beside`): instead of a dragged area, the
    /// ideal size beside the focused window. Places the window waiting for
    /// its place, or keeps the area for the next window (and starts the
    /// command of `area spawn`).
    fn area_beside(&mut self) -> Result<String, String> {
        if !self.area.selecting {
            return Err("not picking an area".into());
        }
        let output = self.active_output().ok_or("no output")?;
        let ws = self.output_ws(&output);
        let waiting = self.area.window;
        let Some(rect) = self.beside_focused(&output, ws, waiting) else {
            // Nothing focused to go beside: the middle, as with Escape.
            self.cancel_area();
            return Ok("nothing focused: the middle of the view".into());
        };
        self.area.selecting = false;
        if self.interaction.is_none() {
            self.cursor_override = None;
        }
        self.center_on_rect(&output, ws, rect);
        if let Some(id) = self.area.window.take() {
            self.show_picked(id, Some((ws, rect)));
            self.pick_next_window();
            return Ok("placed beside the focused window".into());
        }
        self.set_pending_area(ws, rect);
        if let Some(cmd) = self.area.spawn.take() {
            self.note_user_launch();
            self.spawn(&cmd);
        }
        Ok("the next window opens beside the focused one".into())
    }

    /// `new_window_placement = "beside"`: a new window opens at the ideal
    /// size beside the focused one, or stays where map_window put it when
    /// nothing is focused.
    pub fn place_beside(&mut self, idx: usize) {
        let id = self.windows[idx].id;
        let ws = self.windows[idx].ws;
        let output = self
            .outputs
            .iter()
            .find(|o| self.output_ws(o) == ws)
            .cloned()
            .or_else(|| self.active_output());
        let rect = output
            .as_ref()
            .and_then(|o| self.beside_focused(o, ws, Some(id)).map(|r| (o.clone(), r)));
        match rect {
            Some((output, rect)) => {
                self.center_on_rect(&output, ws, rect);
                self.show_picked(id, Some((ws, rect)));
            }
            None => {
                let window = self.windows[idx].window.clone();
                self.focus_window(Some(window));
            }
        }
    }

    /// Frame of the ideal size next to the focused window on `ws`, in the
    /// reading order of window numbers; `None` without a focused canvas
    /// window there. `skip` is the window being placed.
    fn beside_focused(
        &self,
        output: &Output,
        ws: usize,
        skip: Option<u64>,
    ) -> Option<Rectangle<f64, Logical>> {
        let usable = |o: &&crate::shell::Managed| {
            Some(o.id) != skip
                && o.mapped
                && !o.hidden
                && !o.awaiting_pick
                && !o.scratchpad
                && o.ws == ws
                && o.placement == Placement::Canvas
        };
        let focused = self.focused.as_ref()?;
        let anchor = self
            .windows
            .iter()
            .filter(usable)
            .find(|o| &o.window == focused)?;
        let anchor = self.canvas_frame(anchor);
        let size = self.ideal_sizes(output).first()?.size;
        let others: Vec<_> = self
            .windows
            .iter()
            .filter(usable)
            .map(|o| self.canvas_frame(o))
            .collect();
        let gap = (self.config.appearance.gaps as f64).max(MIN_SIBLING_GAP);
        let loc = next_in_order(anchor, size, &others, gap, self.numbering());
        Some(Rectangle::new(loc, size))
    }

    /// Moves the camera of `ws` (at its zoom) so `rect` is in the middle.
    fn center_on_rect(&mut self, output: &Output, ws: usize, rect: Rectangle<f64, Logical>) {
        let area = self.usable_area(output).to_f64();
        let zoom = self.workspace(ws).target().zoom;
        let cam = Camera::centered_on(rect, area, zoom);
        let dur = self.camera_duration();
        self.workspace_mut(ws).animate_to(cam, dur, Instant::now());
        self.queue_redraw(output);
    }

    /// A window that is gone or got its place from a rule needs no pick.
    pub fn forget_pick(&mut self, id: u64) {
        self.area.waiting.retain(|&w| w != id);
        if self.area.window == Some(id) {
            self.area.window = None;
            self.area.selecting = false;
            if self.interaction.is_none() {
                self.cursor_override = None;
            }
            self.queue_redraw_all();
            self.pick_next_window();
        }
    }

    fn set_pending_area(&mut self, ws: usize, rect: Rectangle<f64, Logical>) {
        self.area.pending = Some(PendingArea {
            ws,
            rect,
            until: Instant::now() + AREA_TIMEOUT,
        });
        // Nothing else repaints when it runs out: drop the outline then.
        let _ =
            self.loop_handle
                .insert_source(Timer::from_duration(AREA_TIMEOUT), |_, _, state| {
                    if state
                        .area
                        .pending
                        .is_some_and(|p| Instant::now() >= p.until)
                    {
                        state.area.pending = None;
                        state.queue_redraw_all();
                    }
                    TimeoutAction::Drop
                });
        self.queue_redraw_all();
    }

    /// The drag that picks the area ended at the cursor.
    /// The area from `start` to the cursor, pulled onto a nearby ideal
    /// size (see sizehint.rs); keeps the hint to draw up to date.
    fn dragged_area(
        &mut self,
        output: &Output,
        start: Point<f64, Logical>,
    ) -> Rectangle<f64, Logical> {
        let cam = self.camera(output);
        let end = cam.to_canvas(self.cursor_local(output));
        let raw = span(start, end);
        if self.size_hints_off() {
            self.size_hint = None;
            return raw;
        }
        let ideals = self.ideal_sizes(output);
        let (size, hint) = crate::sizehint::pull(
            raw.size,
            &ideals,
            (true, true),
            crate::sizehint::SNAP_PX / cam.zoom,
            crate::sizehint::SHOW_PX / cam.zoom,
        );
        // Grow away from where the drag started, like the plain rectangle.
        let anchored = |s: Size<f64, Logical>| {
            let x = if end.x >= start.x {
                start.x
            } else {
                start.x - s.w
            };
            let y = if end.y >= start.y {
                start.y
            } else {
                start.y - s.h
            };
            Rectangle::new(Point::from((x, y)), s)
        };
        self.size_hint = hint.map(|(ideal, snapped)| {
            let r = anchored(ideal.size);
            crate::sizehint::SizeHint {
                output: output.clone(),
                rect: Rectangle::new(cam.to_screen(r.loc), r.size.upscale(cam.zoom)),
                ideal,
                snapped,
            }
        });
        anchored(size)
    }

    pub fn area_drag_finished(&mut self, output: &Output, ws: usize, start: Point<f64, Logical>) {
        let cam = self.camera(output);
        let rect = self.dragged_area(output, start);
        self.finish_size_hint();
        let spawn = self.area.spawn.take();
        self.area.selecting = false;
        let small = rect.size.w * cam.zoom < MIN_AREA_PX || rect.size.h * cam.zoom < MIN_AREA_PX;
        if let Some(id) = self.area.window {
            // A click or a tiny drag is no area: the window keeps waiting.
            if small {
                self.area.selecting = true;
                self.cursor_override = Some(CursorIcon::Crosshair);
                return;
            }
            self.area.window = None;
            self.show_picked(id, Some((ws, rect)));
            self.pick_next_window();
            return;
        }
        if small {
            self.cancel_area();
            return;
        }
        self.set_pending_area(ws, rect);
        if let Some(cmd) = spawn {
            self.note_user_launch();
            self.spawn(&cmd);
        }
    }

    /// The area being dragged or waiting on `output`, in output-local
    /// screen units, for drawing it.
    pub fn area_screen_rect(&mut self, output: &Output) -> Option<Rectangle<f64, Logical>> {
        let cam = self.camera(output);
        let to_screen = |r: Rectangle<f64, Logical>| {
            Rectangle::new(cam.to_screen(r.loc), r.size.upscale(cam.zoom))
        };
        if let Some(crate::grabs::Interaction::Area {
            output: o, start, ..
        }) = &self.interaction
        {
            if o == output {
                let start = *start;
                return Some(to_screen(self.dragged_area(output, start)));
            }
        }
        let p = self.area.pending?;
        if Instant::now() >= p.until {
            self.area.pending = None;
            return None;
        }
        (p.ws == self.output_ws(output)).then(|| to_screen(p.rect))
    }

    /// Gives the waiting area to the window being configured for the first
    /// time, unless it is a dialog or a fixed-size window.
    pub fn claim_area(&mut self, idx: usize) {
        let Some(p) = self.area.pending else {
            return;
        };
        if Instant::now() >= p.until {
            self.area.pending = None;
            self.queue_redraw_all();
            return;
        }
        if self.is_dialog_like(idx) {
            return;
        }
        self.area.pending = None;
        self.windows[idx].area = Some((p.ws, p.rect));
        self.queue_redraw_all();
    }

    /// Has a parent, or a fixed size: keeps its own size and place.
    pub fn is_dialog_like(&self, idx: usize) -> bool {
        let Some(toplevel) = self.windows[idx].window.toplevel() else {
            return true;
        };
        if toplevel.parent().is_some() {
            return true;
        }
        let (min, max) = smithay::wayland::compositor::with_states(toplevel.wl_surface(), |s| {
            let mut cached = s
                .cached_state
                .get::<smithay::wayland::shell::xdg::SurfaceCachedState>();
            let current = cached.current();
            (current.min_size, current.max_size)
        });
        max.w > 0 && max.h > 0 && min == max
    }

    /// Top-left (content, below the title) for a new window of an app that
    /// already shows one on `ws`: beside it, or over it for a dialog-sized
    /// window. `None` when the app has no other window there.
    pub fn sibling_spot(&self, idx: usize, ws: usize) -> Option<Point<f64, Logical>> {
        if !self.config.canvas.place_near_siblings {
            return None;
        }
        let m = &self.windows[idx];
        let app = self.app_id(m).filter(|a| !a.is_empty())?;
        // Only the same process opens its own further windows.
        let pid = self.client_pid(m);
        let candidates = || {
            self.windows.iter().filter(|o| {
                o.id != m.id
                    && o.mapped
                    && !o.hidden
                    && !o.awaiting_pick
                    && !o.scratchpad
                    && o.ws == ws
                    && o.placement == Placement::Canvas
                    && self.app_id(o).as_deref() == Some(app.as_str())
                    && self.client_pid(o) == pid
            })
        };
        let focused = self.focused.as_ref();
        let sibling = candidates()
            .find(|o| Some(&o.window) == focused)
            .or_else(|| candidates().max_by_key(|o| o.id))?;
        let sib = self.canvas_frame(sibling);
        let title = self.title_height(m);
        let size = m.size();
        let frame = Size::from((size.w, size.h + title));
        if frame.w <= sib.size.w * DIALOG_SHARE && frame.h <= sib.size.h * DIALOG_SHARE {
            return Some(Point::from((
                sib.loc.x + (sib.size.w - frame.w) / 2.0,
                sib.loc.y + (sib.size.h - frame.h) / 2.0 + title,
            )));
        }
        let others: Vec<_> = self
            .windows
            .iter()
            .filter(|o| {
                o.id != m.id
                    && o.mapped
                    && !o.awaiting_pick
                    && o.ws == ws
                    && o.placement == Placement::Canvas
            })
            .map(|o| self.canvas_frame(o))
            .collect();
        let gap = (self.config.appearance.gaps as f64).max(MIN_SIBLING_GAP);
        let top_left = beside(sib, frame, &others, gap);
        Some(Point::from((top_left.x, top_left.y + title)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn span_orders_the_corners() {
        let r = span((300.0, 50.0).into(), (100.0, 250.0).into());
        assert_eq!(r, rect(100.0, 50.0, 200.0, 200.0));
    }

    #[test]
    fn beside_prefers_the_right_side() {
        let sib = rect(0.0, 0.0, 800.0, 600.0);
        let p = beside(sib, (400.0, 300.0).into(), &[sib], 20.0);
        assert_eq!(p, Point::from((820.0, 0.0)));
    }

    #[test]
    fn beside_goes_below_when_the_right_is_taken() {
        let sib = rect(0.0, 0.0, 800.0, 600.0);
        let right = rect(820.0, 0.0, 800.0, 600.0);
        let p = beside(sib, (400.0, 300.0).into(), &[sib, right], 20.0);
        assert_eq!(p, Point::from((0.0, 620.0)));
    }

    #[test]
    fn beside_falls_back_to_the_right_when_surrounded() {
        let sib = rect(0.0, 0.0, 100.0, 100.0);
        let all = [rect(-1000.0, -1000.0, 3000.0, 3000.0)];
        let p = beside(sib, (50.0, 50.0).into(), &all, 10.0);
        assert_eq!(p, Point::from((110.0, 0.0)));
    }

    #[test]
    fn rows_open_to_the_right_past_windows_in_the_way() {
        let anchor = rect(0.0, 0.0, 1000.0, 600.0);
        let taken = rect(1020.0, 100.0, 500.0, 300.0);
        let p = next_in_order(
            anchor,
            (1000.0, 600.0).into(),
            &[anchor, taken],
            20.0,
            Numbering::Rows,
        );
        assert_eq!(p, Point::from((1540.0, 0.0)));
    }

    #[test]
    fn columns_open_below_and_rtl_to_the_left() {
        let anchor = rect(0.0, 0.0, 1000.0, 600.0);
        let size: Size<f64, Logical> = (800.0, 500.0).into();
        let below = next_in_order(anchor, size, &[anchor], 20.0, Numbering::Columns);
        assert_eq!(below, Point::from((0.0, 620.0)));
        let left = next_in_order(anchor, size, &[anchor], 20.0, Numbering::RowsRtl);
        assert_eq!(left, Point::from((-820.0, 0.0)));
    }

    #[test]
    fn rect_arguments_parse() {
        assert_eq!(
            parse_rect("10,20 300x200"),
            Some(rect(10.0, 20.0, 300.0, 200.0))
        );
        assert_eq!(parse_rect("-5,0 1x1"), Some(rect(-5.0, 0.0, 1.0, 1.0)));
        assert_eq!(parse_rect("10,20 0x200"), None);
        assert_eq!(parse_rect("10 300x200"), None);
        assert_eq!(parse_rect("a,b cxd"), None);
    }
}
