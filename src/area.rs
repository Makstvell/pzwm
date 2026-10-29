//! Where new windows go besides the middle of the view:
//! - an **area** the user drags out on the canvas (`area`, Super+O, or
//!   Super+Ctrl + left drag): the next window that opens fills it;
//! - with `[canvas] new_window_placement = "pick"` every new window waits,
//!   hidden, until its place is dragged out (a click: its own size around
//!   the click, Escape: the middle of the view);
//! - next to a **sibling**: a new window of an app that already has one on
//!   the workspace opens beside it (small ones, like dialogs, over it), the
//!   way KDE keeps an app's windows together.

use std::time::{Duration, Instant};

use smithay::{
    input::pointer::CursorIcon,
    output::Output,
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Logical, Point, Rectangle, Size},
};

use crate::{shell::Placement, state::State};

/// How long a picked area waits for its window.
pub const AREA_TIMEOUT: Duration = Duration::from_secs(60);
/// A drag smaller than this (screen pixels) cancels the pick.
const MIN_AREA_PX: f64 = 40.0;
/// A window at most this share of its sibling both ways counts as a dialog
/// and is centered over it instead of put beside it.
const DIALOG_SHARE: f64 = 0.6;
/// Space between a window and its sibling, canvas units, at least.
const MIN_SIBLING_GAP: f64 = 16.0;

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
            _ => Err("usage: area [spawn CMD] | cancel | rect X,Y WxH".into()),
        }
    }

    fn begin_area_pick(&mut self, spawn: Option<String>) -> Result<String, String> {
        self.area.selecting = true;
        self.area.spawn = spawn;
        self.cursor_override = Some(CursorIcon::Crosshair);
        self.queue_redraw_all();
        Ok("drag an area for the next window (Esc cancels)".into())
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
                quote("Drag its area · click: its size here · Esc: the middle"),
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

    /// Frame size of the window being placed, canvas units.
    fn picked_frame_size(&self) -> Option<Size<f64, Logical>> {
        self.picked_frame_size_of(self.area.window?)
    }

    fn picked_frame_size_of(&self, id: u64) -> Option<Size<f64, Logical>> {
        let m = &self.windows[self.index_of_id(id)?];
        let size = m.size();
        Some(Size::from((size.w, size.h + self.title_height(m))))
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
    pub fn area_drag_finished(&mut self, output: &Output, ws: usize, start: Point<f64, Logical>) {
        let cam = self.camera(output);
        let end = cam.to_canvas(self.cursor_local(output));
        let rect = span(start, end);
        let spawn = self.area.spawn.take();
        self.area.selecting = false;
        let small = rect.size.w * cam.zoom < MIN_AREA_PX || rect.size.h * cam.zoom < MIN_AREA_PX;
        if let Some(id) = self.area.window.take() {
            // A click keeps the window's own size, centered on the click.
            let rect = match (small, self.picked_frame_size_of(id)) {
                (true, Some(size)) => {
                    Rectangle::new((end.x - size.w / 2.0, end.y - size.h / 2.0).into(), size)
                }
                _ => rect,
            };
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
                return Some(to_screen(span(
                    *start,
                    cam.to_canvas(self.cursor_local(output)),
                )));
            }
        }
        // Picking a waiting window's place: its outline follows the cursor.
        if self.area.selecting && self.interaction.is_none() {
            if let Some(size) = self.picked_frame_size() {
                let c = self.cursor_local(output);
                let s = size.upscale(cam.zoom);
                return Some(Rectangle::new((c.x - s.w / 2.0, c.y - s.h / 2.0).into(), s));
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
