// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Pointer interactions owned by the compositor: panning the canvas, moving
//! and resizing windows. They run as a smithay pointer grab so clients see no
//! input while the drag lasts.

use std::time::Duration;

use smithay::{
    input::pointer::{
        AxisFrame, ButtonEvent, CursorIcon, Focus, GestureHoldBeginEvent, GestureHoldEndEvent,
        GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
        GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
        MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
    },
    output::Output,
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_protocols::xdg::shell::server::xdg_toplevel::{ResizeEdge, State as ToplevelState},
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, Serial, Size},
};

use crate::{
    action::Dir,
    canvas::snap_position,
    minimap::MapLayout,
    shell::{Placement, ResizeAnchor},
    state::State,
};

const MIN_W: f64 = 120.0;
const MIN_H: f64 = 60.0;
/// Distance from a screen edge that triggers edge tiling while dragging.
const EDGE_TILE_PX: f64 = 4.0;
/// How far from a screen corner, along its edges, a drop fills the quarter.
const CORNER_ZONE_PX: f64 = 120.0;
const ANCHOR_HOLD: Duration = Duration::from_millis(400);

pub enum Interaction {
    Pan {
        output: Output,
        last: Point<f64, Logical>,
    },
    Move {
        id: u64,
        /// Cursor position relative to the content top-left, window units.
        offset: Point<f64, Logical>,
    },
    /// Dragging the view rectangle around the minimap.
    MapDrag { output: Output, layout: MapLayout },
    /// Moving a whole group (`move-group` mouse bind).
    MoveGroup {
        name: String,
        ws: usize,
        output: Output,
        last: Point<f64, Logical>,
    },
    /// Drawing stroke `id` (or erasing) with the pen, see draw.rs.
    Draw {
        output: Output,
        ws: usize,
        id: u64,
        erase: bool,
    },
    /// Picking cells of the `tile` grid, see tile.rs.
    Tile { output: Output },
    /// Dragging out the area for the next window, see area.rs.
    Area {
        output: Output,
        ws: usize,
        /// Corner where the drag started, canvas units.
        start: Point<f64, Logical>,
    },
    Resize {
        id: u64,
        edges: ResizeEdge,
        start_cursor: Point<f64, Logical>,
        start_pos: Point<f64, Logical>,
        start_size: Size<f64, Logical>,
        scale: f64,
    },
}

pub fn edge_icon(edges: ResizeEdge) -> CursorIcon {
    match edges {
        ResizeEdge::Top => CursorIcon::NResize,
        ResizeEdge::Bottom => CursorIcon::SResize,
        ResizeEdge::Left => CursorIcon::WResize,
        ResizeEdge::Right => CursorIcon::EResize,
        ResizeEdge::TopLeft => CursorIcon::NwResize,
        ResizeEdge::TopRight => CursorIcon::NeResize,
        ResizeEdge::BottomLeft => CursorIcon::SwResize,
        _ => CursorIcon::SeResize,
    }
}

fn begin(
    state: &mut State,
    interaction: Interaction,
    icon: CursorIcon,
    serial: Serial,
    button: u32,
) {
    state.interaction = Some(interaction);
    state.cursor_override = Some(icon);
    let pointer = state.pointer.clone();
    let start_data = pointer.grab_start_data().unwrap_or(GrabStartData {
        focus: None,
        button,
        location: state.cursor,
    });
    pointer.set_grab(state, CompositorGrab { start_data }, serial, Focus::Clear);
    state.queue_redraw_all();
}

pub fn start_pan(state: &mut State, serial: Serial, button: u32) {
    let Some(output) = state.active_output() else {
        return;
    };
    let last = state.cursor;
    begin(
        state,
        Interaction::Pan { output, last },
        CursorIcon::Grabbing,
        serial,
        button,
    );
}

pub fn start_draw(
    state: &mut State,
    output: Output,
    ws: usize,
    id: u64,
    erase: bool,
    serial: Serial,
    button: u32,
) {
    let icon = if erase {
        CursorIcon::NotAllowed
    } else {
        CursorIcon::Crosshair
    };
    begin(
        state,
        Interaction::Draw {
            output,
            ws,
            id,
            erase,
        },
        icon,
        serial,
        button,
    );
}

/// Starts picking cells of the open `tile` grid at the cursor.
pub fn start_tile(state: &mut State, serial: Serial, button: u32) {
    let Some(output) = state.tile.as_ref().map(|t| t.output.clone()) else {
        return;
    };
    state.tile_drag_to_cursor(true);
    begin(
        state,
        Interaction::Tile { output },
        CursorIcon::Crosshair,
        serial,
        button,
    );
}

/// Starts dragging out the area the next window opens in.
pub fn start_area(state: &mut State, serial: Serial, button: u32) -> bool {
    let Some(output) = state.active_output() else {
        return false;
    };
    let ws = state.output_ws(&output);
    let start = state.camera(&output).to_canvas(state.cursor_local(&output));
    begin(
        state,
        Interaction::Area { output, ws, start },
        CursorIcon::Crosshair,
        serial,
        button,
    );
    true
}

/// Starts dragging the focused window's group; a window outside any group
/// is moved alone. Returns false when nothing is focused.
pub fn start_group_move(state: &mut State, serial: Serial, button: u32) -> bool {
    let Some(idx) = state.focused_index() else {
        return false;
    };
    let m = &state.windows[idx];
    let (Some(name), Placement::Canvas) = (m.group.clone(), m.placement) else {
        start_move_with_button(state, idx, serial, button);
        return true;
    };
    let ws = m.ws;
    let Some(output) = state.active_output() else {
        return false;
    };
    let last = state.cursor;
    begin(
        state,
        Interaction::MoveGroup {
            name,
            ws,
            output,
            last,
        },
        CursorIcon::Grabbing,
        serial,
        button,
    );
    true
}

pub fn start_map_drag(state: &mut State, output: Output, layout: MapLayout, serial: Serial) {
    begin(
        state,
        Interaction::MapDrag { output, layout },
        CursorIcon::Grabbing,
        serial,
        0x110,
    );
}

pub fn start_move(state: &mut State, idx: usize, serial: Serial) {
    start_move_with_button(state, idx, serial, 0x110);
}

pub fn start_move_with_button(state: &mut State, idx: usize, serial: Serial, button: u32) {
    let Some(output) = state.active_output() else {
        return;
    };
    let local_screen = state.cursor_local(&output);
    // Placed by hand now: rules that match late must not move it back.
    state.windows[idx].late_rules = None;
    let m = &state.windows[idx];
    let id = m.id;

    let offset = if matches!(m.placement, Placement::Maximized | Placement::Fullscreen) {
        // Dragging a maximized window pops it back out under the cursor,
        // keeping the grab point at the same relative x.
        let Some(view) = state.view(m, &output) else {
            return;
        };
        let frac = (view.to_local(local_screen).x / m.size().w.max(1.0)).clamp(0.05, 0.95);
        let title = state.title_height(m).max(16.0);
        state.restore(idx);
        let idx = state.index_of_id(id).unwrap_or(idx);
        let m = &state.windows[idx];
        let size = m.size();
        let offset = Point::from((frac * size.w, -title / 2.0));
        let canvas = m.placement == Placement::Canvas;
        let pos = if canvas {
            state.camera(&output).to_canvas(local_screen) - offset
        } else {
            local_screen - offset
        };
        let m = &mut state.windows[idx];
        if canvas {
            m.ws = state_output_ws(&output);
        }
        m.pos = pos;
        offset
    } else {
        let Some(view) = state.view(m, &output) else {
            return;
        };
        view.to_local(local_screen)
    };
    if let Some(m) = state.windows.iter_mut().find(|m| m.id == id) {
        m.anchor = None;
    }
    begin(
        state,
        Interaction::Move { id, offset },
        CursorIcon::Grabbing,
        serial,
        button,
    );
}

fn state_output_ws(output: &Output) -> usize {
    crate::state::output_state(output).ws
}

pub fn start_resize(state: &mut State, idx: usize, edges: ResizeEdge, serial: Serial) {
    start_resize_with_button(state, idx, edges, serial, 0x110);
}

pub fn start_resize_with_button(
    state: &mut State,
    idx: usize,
    edges: ResizeEdge,
    serial: Serial,
    button: u32,
) {
    let Some(output) = state.active_output() else {
        return;
    };
    let m = &state.windows[idx];
    if !matches!(m.placement, Placement::Canvas | Placement::Pinned) {
        return;
    }
    let Some(view) = state.view(m, &output) else {
        return;
    };
    let interaction = Interaction::Resize {
        id: m.id,
        edges,
        start_cursor: state.cursor,
        start_pos: m.pos,
        start_size: m.size(),
        scale: view.scale,
    };
    if let Some(t) = m.window.toplevel() {
        t.with_pending_state(|s| s.states.set(ToplevelState::Resizing));
    }
    begin(state, interaction, edge_icon(edges), serial, button);
}

/// Picks the resize edge for the resize mouse bind from where the window was
/// grabbed: outer thirds pick that side, corners resize diagonally, the
/// middle resizes the bottom-right corner.
pub fn nearest_edges(local: Point<f64, Logical>, size: Size<f64, Logical>) -> ResizeEdge {
    let horizontal = if local.x < size.w / 3.0 {
        Some(true)
    } else if local.x > size.w * 2.0 / 3.0 {
        Some(false)
    } else {
        None
    };
    let vertical = if local.y < size.h / 3.0 {
        Some(true)
    } else if local.y > size.h * 2.0 / 3.0 {
        Some(false)
    } else {
        None
    };
    match (horizontal, vertical) {
        (Some(true), Some(true)) => ResizeEdge::TopLeft,
        (Some(true), Some(false)) => ResizeEdge::BottomLeft,
        (Some(false), Some(true)) => ResizeEdge::TopRight,
        (Some(true), None) => ResizeEdge::Left,
        (Some(false), None) => ResizeEdge::Right,
        (None, Some(true)) => ResizeEdge::Top,
        (None, Some(false)) => ResizeEdge::Bottom,
        (Some(false), Some(false)) | (None, None) => ResizeEdge::BottomRight,
    }
}

impl State {
    /// Applies the current drag to the canvas or window.
    pub fn interaction_motion(&mut self) {
        let Some(interaction) = self.interaction.take() else {
            return;
        };
        let interaction = match interaction {
            Interaction::Pan { output, last } => {
                let delta = self.cursor - last;
                self.pan(&output, delta, false);
                Interaction::Pan {
                    output,
                    last: self.cursor,
                }
            }
            Interaction::Move { id, offset } => {
                self.drag_window(id, offset);
                Interaction::Move { id, offset }
            }
            Interaction::MapDrag { output, layout } => {
                let local = self.cursor_local(&output);
                self.minimap_jump(&output, &layout, local, false);
                Interaction::MapDrag { output, layout }
            }
            Interaction::MoveGroup {
                name,
                ws,
                output,
                last,
            } => {
                let zoom = self.camera(&output).zoom;
                let delta = (self.cursor - last).downscale(zoom);
                self.move_group_by(&name, ws, delta);
                Interaction::MoveGroup {
                    name,
                    ws,
                    output,
                    last: self.cursor,
                }
            }
            Interaction::Draw {
                output,
                ws,
                id,
                erase,
            } => {
                if erase {
                    self.erase_at_cursor(ws);
                } else {
                    self.draw_motion(&output, ws, id);
                }
                Interaction::Draw {
                    output,
                    ws,
                    id,
                    erase,
                }
            }
            Interaction::Area { output, ws, start } => {
                self.queue_redraw(&output);
                Interaction::Area { output, ws, start }
            }
            Interaction::Tile { output } => {
                self.tile_drag_to_cursor(false);
                Interaction::Tile { output }
            }
            Interaction::Resize {
                id,
                edges,
                start_cursor,
                start_pos,
                start_size,
                scale,
            } => {
                self.drag_resize(id, edges, start_cursor, start_pos, start_size, scale);
                Interaction::Resize {
                    id,
                    edges,
                    start_cursor,
                    start_pos,
                    start_size,
                    scale,
                }
            }
        };
        self.interaction = Some(interaction);
    }

    fn drag_window(&mut self, id: u64, offset: Point<f64, Logical>) {
        let Some(idx) = self.index_of_id(id) else {
            return;
        };
        let Some(output) = self.output_at(self.cursor).or_else(|| self.active_output()) else {
            return;
        };
        let local = self.cursor_local(&output);
        let ws = self.output_ws(&output);

        if self.windows[idx].placement == Placement::Pinned {
            let m = &mut self.windows[idx];
            m.pos = local - offset;
            m.output = Some(output.name());
            self.queue_redraw_all();
            return;
        }

        let cam = self.camera(&output);
        let title = self.title_height(&self.windows[idx]);
        let size = self.windows[idx].size();
        let wanted = cam.to_canvas(local) - offset;
        let frame = Rectangle::new(
            Point::from((wanted.x, wanted.y - title)),
            Size::from((size.w, size.h + title)),
        );
        let mut targets: Vec<Rectangle<f64, Logical>> = self
            .windows
            .iter()
            .filter(|o| {
                o.id != id && self.shown(o) && o.ws == ws && o.placement == Placement::Canvas
            })
            .map(|o| self.canvas_frame(o))
            .collect();
        targets.push(cam.visible(self.usable_area(&output).to_f64()));
        let threshold = self.config.canvas.snap_distance / cam.zoom;
        let gap = self.config.appearance.gaps as f64;
        let snapped = snap_position(frame, &targets, threshold, gap);

        let m = &mut self.windows[idx];
        m.ws = ws;
        m.pos = Point::from((snapped.x, snapped.y + title));
        self.snap_preview = self.edge_tile_target(&output, local);
        self.queue_redraw_all();
    }

    /// Screen rectangle a window dropped at `local` would tile into.
    fn edge_tile_target(
        &self,
        output: &Output,
        local: Point<f64, Logical>,
    ) -> Option<(Output, Rectangle<f64, Logical>)> {
        if !self.config.canvas.edge_tiling {
            return None;
        }
        let size = self.output_geometry(output).size.to_f64();
        let (left, right) = (local.x <= EDGE_TILE_PX, local.x >= size.w - EDGE_TILE_PX);
        let (top, bottom) = (local.y <= EDGE_TILE_PX, local.y >= size.h - EDGE_TILE_PX);
        // Near a corner (on either of its edges): that quarter of the screen.
        let near_left = local.x <= CORNER_ZONE_PX;
        let near_right = local.x >= size.w - CORNER_ZONE_PX;
        let near_top = local.y <= CORNER_ZONE_PX;
        let near_bottom = local.y >= size.h - CORNER_ZONE_PX;
        let corner = match () {
            _ if (left || top) && near_left && near_top => Some((true, true)),
            _ if (right || top) && near_right && near_top => Some((false, true)),
            _ if (left || bottom) && near_left && near_bottom => Some((true, false)),
            _ if (right || bottom) && near_right && near_bottom => Some((false, false)),
            _ => None,
        };
        if let Some((l, t)) = corner {
            let area = self.usable_area(output).to_f64();
            let gap = self.config.appearance.gaps as f64;
            return Some((output.clone(), crate::tile::quarter_rect(area, gap, l, t)));
        }
        let rect = if left {
            self.half_rect(output, Dir::Left)
        } else if right {
            self.half_rect(output, Dir::Right)
        } else if top {
            let area = self.usable_area(output).to_f64();
            let gap = self.config.appearance.gaps as f64;
            Rectangle::new(
                area.loc + Point::from((gap, gap)),
                Size::from((area.size.w - 2.0 * gap, area.size.h - 2.0 * gap)),
            )
        } else {
            return None;
        };
        Some((output.clone(), rect))
    }

    fn drag_resize(
        &mut self,
        id: u64,
        edges: ResizeEdge,
        start_cursor: Point<f64, Logical>,
        start_pos: Point<f64, Logical>,
        start_size: Size<f64, Logical>,
        scale: f64,
    ) {
        let Some(idx) = self.index_of_id(id) else {
            return;
        };
        let d = (self.cursor - start_cursor).downscale(scale);
        let (mut w, mut h) = (start_size.w, start_size.h);
        let has = |e: &[ResizeEdge]| e.contains(&edges);
        if has(&[
            ResizeEdge::Right,
            ResizeEdge::TopRight,
            ResizeEdge::BottomRight,
        ]) {
            w += d.x;
        }
        if has(&[
            ResizeEdge::Left,
            ResizeEdge::TopLeft,
            ResizeEdge::BottomLeft,
        ]) {
            w -= d.x;
        }
        if has(&[
            ResizeEdge::Bottom,
            ResizeEdge::BottomLeft,
            ResizeEdge::BottomRight,
        ]) {
            h += d.y;
        }
        if has(&[ResizeEdge::Top, ResizeEdge::TopLeft, ResizeEdge::TopRight]) {
            h -= d.y;
        }
        // Ideal sizes pull the edges being dragged, see sizehint.rs.
        let axes = (
            has(&[
                ResizeEdge::Left,
                ResizeEdge::Right,
                ResizeEdge::TopLeft,
                ResizeEdge::TopRight,
                ResizeEdge::BottomLeft,
                ResizeEdge::BottomRight,
            ]),
            has(&[
                ResizeEdge::Top,
                ResizeEdge::Bottom,
                ResizeEdge::TopLeft,
                ResizeEdge::TopRight,
                ResizeEdge::BottomLeft,
                ResizeEdge::BottomRight,
            ]),
        );
        self.size_hint = None;
        if let (false, Some(output)) = (self.size_hints_off(), self.output_of(idx)) {
            let title = self.title_height(&self.windows[idx]);
            let ideals = self.ideal_sizes(&output);
            let (frame, hint) = crate::sizehint::pull(
                Size::from((w, h + title)),
                &ideals,
                axes,
                crate::sizehint::SNAP_PX / scale,
                crate::sizehint::SHOW_PX / scale,
            );
            w = frame.w;
            h = frame.h - title;
            if let Some((ideal, snapped)) = hint {
                // The ideal frame where the resized one would be: the edges
                // not dragged stay where they started.
                let iw = if axes.0 { ideal.size.w } else { w };
                let ih = if axes.1 { ideal.size.h - title } else { h };
                let x = if has(&[
                    ResizeEdge::Left,
                    ResizeEdge::TopLeft,
                    ResizeEdge::BottomLeft,
                ]) {
                    start_pos.x + start_size.w - iw
                } else {
                    start_pos.x
                };
                let y = if has(&[ResizeEdge::Top, ResizeEdge::TopLeft, ResizeEdge::TopRight]) {
                    start_pos.y + start_size.h - ih
                } else {
                    start_pos.y
                };
                let frame =
                    Rectangle::new(Point::from((x, y - title)), Size::from((iw, ih + title)));
                let rect = if self.windows[idx].placement == Placement::Pinned {
                    frame
                } else {
                    let cam = self.camera(&output);
                    Rectangle::new(cam.to_screen(frame.loc), frame.size.upscale(cam.zoom))
                };
                self.size_hint = Some(crate::sizehint::SizeHint {
                    output,
                    rect,
                    ideal,
                    snapped,
                });
            }
        }
        let size = Size::from((w.max(MIN_W).round() as i32, h.max(MIN_H).round() as i32));
        let m = &mut self.windows[idx];
        m.anchor = Some(ResizeAnchor {
            edges,
            right: start_pos.x + start_size.w,
            bottom: start_pos.y + start_size.h,
        });
        if let Some(t) = m.window.toplevel() {
            t.with_pending_state(|s| {
                s.size = Some(size);
                s.states.set(ToplevelState::Resizing);
            });
            t.send_pending_configure();
        }
    }

    /// Called when the grab ends (all buttons released).
    pub fn end_interaction(&mut self) {
        self.cursor_override = None;
        let preview = self.snap_preview.take();
        match self.interaction.take() {
            Some(Interaction::Move { id, .. }) => {
                if let (Some(idx), Some((output, rect))) = (self.index_of_id(id), preview) {
                    let area = self.usable_area(&output).to_f64();
                    if (rect.size.w - area.size.w).abs() < (area.size.w / 4.0) {
                        self.maximize(idx);
                    } else {
                        self.place_in_screen_rect(idx, &output, rect);
                    }
                }
            }
            Some(Interaction::Resize { id, .. }) => {
                self.finish_size_hint();
                if let Some(idx) = self.index_of_id(id) {
                    if let Some(t) = self.windows[idx].window.toplevel() {
                        t.with_pending_state(|s| s.states.unset(ToplevelState::Resizing));
                        t.send_pending_configure();
                    }
                    // Late commits still belong to this resize; drop the anchor after.
                    let _ = self.loop_handle.insert_source(
                        Timer::from_duration(ANCHOR_HOLD),
                        move |_, _, state| {
                            if let Some(idx) = state.index_of_id(id) {
                                if state.interaction.is_none() {
                                    state.windows[idx].anchor = None;
                                }
                            }
                            TimeoutAction::Drop
                        },
                    );
                }
            }
            Some(Interaction::MapDrag { .. }) => self.map_drag_ended(),
            Some(Interaction::Draw {
                output,
                ws,
                id,
                erase,
            }) => {
                if erase {
                    self.erase_finished();
                } else {
                    self.draw_finished(&output, ws, id);
                }
            }
            Some(Interaction::Area { output, ws, start }) => {
                self.area_drag_finished(&output, ws, start)
            }
            Some(Interaction::Tile { .. }) => self.tile_apply(),
            Some(Interaction::Pan { .. } | Interaction::MoveGroup { .. }) | None => {}
        }
        // We are inside the pointer's grab callback: touching the pointer
        // here would deadlock, so refresh focus once the grab is gone.
        self.loop_handle
            .insert_idle(|state| state.update_pointer_focus());
        self.queue_redraw_all();
    }
}

pub struct CompositorGrab {
    start_data: GrabStartData<State>,
}

impl PointerGrab<State> for CompositorGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        data.interaction_motion();
    }

    fn relative_motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, None, event);
    }

    fn button(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().is_empty() {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        details: AxisFrame,
    ) {
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event);
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event);
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event);
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &GrabStartData<State> {
        &self.start_data
    }

    fn unset(&mut self, data: &mut State) {
        data.end_interaction();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_edges_picks_side_in_the_middle_thirds() {
        let size = Size::from((300.0, 300.0));
        assert_eq!(
            nearest_edges(Point::from((10.0, 150.0)), size),
            ResizeEdge::Left
        );
        assert_eq!(
            nearest_edges(Point::from((150.0, 290.0)), size),
            ResizeEdge::Bottom
        );
        assert_eq!(
            nearest_edges(Point::from((150.0, 150.0)), size),
            ResizeEdge::BottomRight
        );
    }

    #[test]
    fn nearest_edges_picks_quadrant() {
        let size = Size::from((200.0, 100.0));
        assert_eq!(
            nearest_edges(Point::from((10.0, 10.0)), size),
            ResizeEdge::TopLeft
        );
        assert_eq!(
            nearest_edges(Point::from((190.0, 90.0)), size),
            ResizeEdge::BottomRight
        );
        assert_eq!(
            nearest_edges(Point::from((150.0, 20.0)), size),
            ResizeEdge::TopRight
        );
        assert_eq!(
            nearest_edges(Point::from((20.0, 80.0)), size),
            ResizeEdge::BottomLeft
        );
    }
}
