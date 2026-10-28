//! Pointer and touchpad input: focus, canvas gestures, window dragging.
//! Keyboard handling lives in `bindings.rs`.

use crate::{
    grabs,
    shell::{Hit, Hover, Placement},
    state::State,
};
use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, GestureBeginEvent,
        GestureEndEvent, GesturePinchUpdateEvent as _, GestureSwipeUpdateEvent as _, InputBackend,
        InputEvent, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    desktop::{WindowSurfaceType, layer_map_for_output},
    input::pointer::{
        AxisFrame, ButtonEvent, CursorIcon, GesturePinchBeginEvent, GesturePinchEndEvent,
        GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
        GestureSwipeUpdateEvent, MotionEvent, RelativeMotionEvent,
    },
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
    wayland::{
        compositor::with_states,
        pointer_constraints::{PointerConstraint, with_pointer_constraint},
        shell::wlr_layer::{KeyboardInteractivity, Layer, LayerSurfaceCachedState},
    },
};

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

/// What the pointer is over.
pub enum Under {
    /// A client surface; location and surface origin share one space.
    Surface {
        surface: WlSurface,
        surface_loc: Point<f64, Logical>,
        location: Point<f64, Logical>,
        window: Option<usize>,
        layer: Option<Layer>,
    },
    /// Compositor-drawn part of a window (titlebar, button, edge).
    Decoration {
        window: usize,
        hit: Hit,
    },
    Empty,
}

impl State {
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        match event {
            InputEvent::Keyboard { event } => {
                self.handle_key(event.key_code(), event.state(), event.time_msec());
            }
            InputEvent::PointerMotion { event } => {
                self.handle_relative_motion(
                    event.delta(),
                    event.delta_unaccel(),
                    event.time(),
                    event.time_msec(),
                );
            }
            InputEvent::PointerMotionAbsolute { event } => {
                if let Some(output) = self.outputs.first().cloned() {
                    let geo = self.output_geometry(&output);
                    self.cursor = geo.loc.to_f64() + event.position_transformed(geo.size);
                    self.pointer_moved(event.time_msec());
                }
            }
            InputEvent::PointerButton { event } => {
                self.handle_button(event.button_code(), event.state(), event.time_msec());
            }
            InputEvent::PointerAxis { event } => self.handle_axis::<B>(&event),
            InputEvent::GestureSwipeBegin { event } => {
                self.notify_activity();
                if event.fingers() >= 3 && !self.lock.is_locked() {
                    self.swipe = Some(Point::from((0.0, 0.0)));
                } else {
                    let serial = self.next_serial();
                    let pointer = self.pointer.clone();
                    pointer.gesture_swipe_begin(
                        self,
                        &GestureSwipeBeginEvent {
                            serial,
                            time: event.time_msec(),
                            fingers: event.fingers(),
                        },
                    );
                }
            }
            InputEvent::GestureSwipeUpdate { event } => {
                if self.swipe.is_some() {
                    if let Some(output) = self.active_output() {
                        self.pan(&output, event.delta(), false);
                        self.update_pointer_focus();
                    }
                } else {
                    let pointer = self.pointer.clone();
                    pointer.gesture_swipe_update(
                        self,
                        &GestureSwipeUpdateEvent {
                            time: event.time_msec(),
                            delta: event.delta(),
                        },
                    );
                }
            }
            InputEvent::GestureSwipeEnd { event } => {
                if self.swipe.take().is_none() {
                    let serial = self.next_serial();
                    let pointer = self.pointer.clone();
                    pointer.gesture_swipe_end(
                        self,
                        &GestureSwipeEndEvent {
                            serial,
                            time: event.time_msec(),
                            cancelled: event.cancelled(),
                        },
                    );
                }
            }
            InputEvent::GesturePinchBegin { event } => {
                self.notify_activity();
                let canvas_gesture = !self.lock.is_locked()
                    && (self.super_held()
                        || matches!(self.under(), Under::Empty | Under::Decoration { .. }));
                if canvas_gesture {
                    self.pinch = Some(1.0);
                } else {
                    let serial = self.next_serial();
                    let pointer = self.pointer.clone();
                    pointer.gesture_pinch_begin(
                        self,
                        &GesturePinchBeginEvent {
                            serial,
                            time: event.time_msec(),
                            fingers: event.fingers(),
                        },
                    );
                }
            }
            InputEvent::GesturePinchUpdate { event } => {
                if let Some(last) = self.pinch {
                    let scale = event.scale().max(0.01);
                    if let Some(output) = self.active_output() {
                        let anchor = self.cursor_local(&output);
                        self.zoom_at(&output, anchor, scale / last, false);
                        self.update_pointer_focus();
                    }
                    self.pinch = Some(scale);
                } else {
                    let pointer = self.pointer.clone();
                    pointer.gesture_pinch_update(
                        self,
                        &GesturePinchUpdateEvent {
                            time: event.time_msec(),
                            delta: event.delta(),
                            scale: event.scale(),
                            rotation: event.rotation(),
                        },
                    );
                }
            }
            InputEvent::GesturePinchEnd { event } => {
                let canvas_gesture = self.pinch.take().is_some();
                if !canvas_gesture {
                    let serial = self.next_serial();
                    let pointer = self.pointer.clone();
                    pointer.gesture_pinch_end(
                        self,
                        &GesturePinchEndEvent {
                            serial,
                            time: event.time_msec(),
                            cancelled: event.cancelled(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    pub fn notify_activity(&mut self) {
        let seat = self.seat.clone();
        self.idle_notifier_state.notify_activity(&seat);
    }

    fn super_held(&self) -> bool {
        self.seat
            .get_keyboard()
            .is_some_and(|k| k.modifier_state().logo)
    }

    // ---- pointer -----------------------------------------------------------

    fn handle_relative_motion(
        &mut self,
        delta: Point<f64, Logical>,
        delta_unaccel: Point<f64, Logical>,
        utime: u64,
        time: u32,
    ) {
        self.notify_activity();
        let pointer = self.pointer.clone();
        let under = self.under();
        let focus = self.focus_of(&under);

        // Games and 3D apps: a locked pointer only produces relative motion.
        let mut locked = false;
        let mut confined = false;
        if let Some((surface, _)) = &focus {
            with_pointer_constraint(surface, &pointer, |constraint| {
                if let Some(c) = constraint.filter(|c| c.is_active()) {
                    match &*c {
                        PointerConstraint::Locked(_) => locked = true,
                        PointerConstraint::Confined(_) => confined = true,
                    }
                }
            });
        }
        pointer.relative_motion(
            self,
            focus.clone(),
            &RelativeMotionEvent {
                delta,
                delta_unaccel,
                utime,
            },
        );
        if locked {
            pointer.frame(self);
            return;
        }

        let previous = self.cursor;
        self.cursor += delta;
        self.clamp_cursor();
        if confined {
            let still_inside = match (self.focus_of(&self.under()), &focus) {
                (Some((now, _)), Some((before, _))) => &now == before,
                _ => false,
            };
            if !still_inside {
                self.cursor = previous;
                pointer.frame(self);
                return;
            }
        }
        self.pointer_moved(time);
    }

    fn pointer_moved(&mut self, time: u32) {
        self.check_pending_middle();
        let pointer = self.pointer.clone();
        let under = self.under();
        self.update_hover(&under);
        let focus = self.focus_of(&under);
        let location = self.event_location(&under);
        let serial = self.next_serial();
        pointer.motion(
            self,
            focus.clone(),
            &MotionEvent {
                location,
                serial,
                time,
            },
        );
        pointer.frame(self);

        if let Some((surface, surface_loc)) = focus {
            with_pointer_constraint(&surface, &pointer, |constraint| {
                if let Some(c) = constraint.filter(|c| !c.is_active()) {
                    let point = (location - surface_loc).to_i32_round();
                    if c.region().is_none_or(|r| r.contains(point)) {
                        c.activate();
                    }
                }
            });
        }
        // The cursor is drawn by us: every move needs a frame.
        if let Some(output) = self.output_at(self.cursor) {
            self.queue_redraw(&output);
        }
    }

    /// Re-evaluates what is under a cursor that did not move (the canvas or
    /// a window moved instead).
    pub fn update_pointer_focus(&mut self) {
        if self.pointer.is_grabbed() {
            return;
        }
        let time = self.now_ms();
        self.pointer_moved(time);
    }

    fn focus_of(&self, under: &Under) -> Option<(WlSurface, Point<f64, Logical>)> {
        match under {
            Under::Surface {
                surface,
                surface_loc,
                ..
            } => Some((surface.clone(), *surface_loc)),
            _ => None,
        }
    }

    fn event_location(&self, under: &Under) -> Point<f64, Logical> {
        match under {
            Under::Surface { location, .. } => *location,
            _ => self.cursor,
        }
    }

    fn update_hover(&mut self, under: &Under) {
        let (hover, icon) = match under {
            Under::Decoration { window, hit } => {
                let id = self.windows[*window].id;
                match hit {
                    Hit::Button(button) => (
                        Some(Hover {
                            id,
                            button: *button,
                        }),
                        Some(CursorIcon::Pointer),
                    ),
                    Hit::Edge(edges) => (None, Some(grabs::edge_icon(*edges))),
                    _ => (None, Some(CursorIcon::Default)),
                }
            }
            Under::Empty => (None, Some(CursorIcon::Default)),
            Under::Surface { .. } => (None, None),
        };
        if self.interaction.is_none() {
            self.cursor_override = icon;
        }
        if self.hover != hover {
            self.hover = hover;
            self.queue_redraw_all();
        }
    }

    /// Hit test in stacking order: lock, overlay, fullscreen, top layer,
    /// windows, bottom layer. The wallpaper (background layer) never takes
    /// the pointer, so the canvas stays draggable everywhere.
    pub fn under(&self) -> Under {
        let Some(output) = self.output_at(self.cursor) else {
            return Under::Empty;
        };
        let local = self.cursor_local(&output);

        if self.lock.is_locked() {
            return match self.lock_surfaces.get(&output.name()) {
                Some(lock) => Under::Surface {
                    surface: lock.wl_surface().clone(),
                    surface_loc: Point::from((0.0, 0.0)),
                    location: local,
                    window: None,
                    layer: None,
                },
                None => Under::Empty,
            };
        }

        if let Some(under) = self.layer_under(&output, Layer::Overlay, local) {
            return under;
        }
        let fullscreen = self
            .window_hit(&output, local)
            .filter(|(i, _)| self.windows[*i].placement == Placement::Fullscreen);
        if let Some((i, hit)) = fullscreen {
            return window_under(i, hit);
        }
        let has_fullscreen = self
            .windows
            .iter()
            .any(|m| m.placement == Placement::Fullscreen && self.view(m, &output).is_some());
        if !has_fullscreen {
            if let Some(under) = self.layer_under(&output, Layer::Top, local) {
                return under;
            }
        }
        if let Some((i, hit)) = self.window_hit(&output, local) {
            return window_under(i, hit);
        }
        if let Some(under) = self.layer_under(&output, Layer::Bottom, local) {
            return under;
        }
        Under::Empty
    }

    fn layer_under(
        &self,
        output: &Output,
        layer: Layer,
        local: Point<f64, Logical>,
    ) -> Option<Under> {
        let map = layer_map_for_output(output);
        let surface = map.layer_under(layer, local)?;
        let geo = map.layer_geometry(surface)?;
        let (wl, loc) = surface.surface_under(local - geo.loc.to_f64(), WindowSurfaceType::ALL)?;
        Some(Under::Surface {
            surface: wl,
            surface_loc: (geo.loc + loc).to_f64(),
            location: local,
            window: None,
            layer: Some(layer),
        })
    }

    fn handle_button(&mut self, button: u32, button_state: ButtonState, time: u32) {
        self.notify_activity();
        let serial = self.next_serial();
        let pointer = self.pointer.clone();
        let event = ButtonEvent {
            button,
            state: button_state,
            serial,
            time,
        };
        let free = !pointer.is_grabbed() && !self.lock.is_locked();

        // Middle button over a window: wait to see whether it is a click
        // (sent to the app: paste, open link) or a drag (pans the canvas).
        if button == BTN_MIDDLE && free {
            match button_state {
                ButtonState::Pressed
                    if self.config.canvas.middle_drag_over_windows
                        && !self.super_held()
                        && matches!(
                            self.under(),
                            Under::Surface {
                                window: Some(_),
                                ..
                            }
                        ) =>
                {
                    self.pending_middle = Some((self.cursor, time));
                    return;
                }
                ButtonState::Released => {
                    if let Some((_, pressed_at)) = self.pending_middle.take() {
                        let press = ButtonEvent {
                            button,
                            state: ButtonState::Pressed,
                            serial,
                            time: pressed_at,
                        };
                        pointer.button(self, &press);
                        pointer.frame(self);
                        let release = ButtonEvent {
                            serial: self.next_serial(),
                            ..event
                        };
                        pointer.button(self, &release);
                        pointer.frame(self);
                        return;
                    }
                }
                _ => {}
            }
        }

        if button_state == ButtonState::Pressed && free {
            self.press(button, serial);
        }
        pointer.button(self, &event);
        pointer.frame(self);
    }

    /// Turns a held middle button into a canvas pan once it moves far enough.
    fn check_pending_middle(&mut self) {
        const DRAG_START_PX: f64 = 6.0;
        let Some((start, _)) = self.pending_middle else {
            return;
        };
        let d = self.cursor - start;
        if d.x * d.x + d.y * d.y >= DRAG_START_PX * DRAG_START_PX {
            self.pending_middle = None;
            let serial = self.next_serial();
            grabs::start_pan(self, serial, BTN_MIDDLE);
            if let Some(grabs::Interaction::Pan { last, .. }) = &mut self.interaction {
                *last = start;
            }
            self.interaction_motion();
        }
    }

    /// Compositor reaction to a press; grabs started here receive the button.
    fn press(&mut self, button: u32, serial: smithay::utils::Serial) {
        let super_held = self.super_held();
        match self.under() {
            Under::Decoration { window, .. }
            | Under::Surface {
                window: Some(window),
                ..
            } if super_held => {
                let w = self.windows[window].window.clone();
                self.focus_window(Some(w));
                self.super_drag(button, serial);
            }
            Under::Surface {
                window: Some(idx), ..
            } => {
                let window = self.windows[idx].window.clone();
                if self.focused.as_ref() != Some(&window) || self.layer_focus.is_some() {
                    self.focus_window(Some(window));
                }
            }
            Under::Surface {
                surface,
                layer: Some(_),
                ..
            } => {
                let interactive = with_states(&surface, |states| {
                    states
                        .cached_state
                        .get::<LayerSurfaceCachedState>()
                        .current()
                        .keyboard_interactivity
                        != KeyboardInteractivity::None
                });
                if interactive {
                    self.layer_focus = Some(surface);
                    self.refresh_keyboard_focus();
                }
            }
            Under::Surface { .. } => {}
            Under::Decoration { window, hit } => {
                let w = self.windows[window].window.clone();
                let id = self.windows[window].id;
                self.focus_window(Some(w));
                let Some(idx) = self.index_of_id(id) else {
                    return;
                };
                match (hit, button) {
                    (Hit::Button(b), BTN_LEFT) => self.press_button(idx, b),
                    (Hit::Title, BTN_LEFT) => {
                        if self.title_clicked(id) {
                            self.toggle_maximize(idx);
                        } else {
                            grabs::start_move_with_button(self, idx, serial, button);
                        }
                    }
                    (Hit::Title, BTN_MIDDLE) => grabs::start_pan(self, serial, button),
                    (Hit::Edge(edges), BTN_LEFT) => {
                        grabs::start_resize_with_button(self, idx, edges, serial, button)
                    }
                    _ => {}
                }
            }
            Under::Empty => {
                let pans = button == BTN_MIDDLE
                    || (button == BTN_LEFT && self.config.canvas.left_drag_pans);
                if pans || (super_held && button == BTN_LEFT) {
                    grabs::start_pan(self, serial, button);
                }
            }
        }
    }

    /// Super + drag: left moves, right resizes, middle pans.
    fn super_drag(&mut self, button: u32, serial: smithay::utils::Serial) {
        let Some(idx) = self.focused_index() else {
            return;
        };
        match button {
            BTN_LEFT => grabs::start_move_with_button(self, idx, serial, button),
            BTN_RIGHT => {
                let Some(output) = self.active_output() else {
                    return;
                };
                let Some(view) = self.view(&self.windows[idx], &output) else {
                    return;
                };
                let local = view.to_local(self.cursor_local(&output));
                let edges = grabs::nearest_edges(local, self.windows[idx].size());
                grabs::start_resize_with_button(self, idx, edges, serial, button);
            }
            BTN_MIDDLE => grabs::start_pan(self, serial, button),
            _ => {}
        }
    }

    fn handle_axis<B: InputBackend>(&mut self, event: &B::PointerAxisEvent) {
        self.notify_activity();
        let source = event.source();
        let h = event
            .amount(Axis::Horizontal)
            .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.0);
        let v = event
            .amount(Axis::Vertical)
            .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.0);

        if self.canvas_scroll_allowed() {
            if let Some(output) = self.active_output() {
                self.scroll_canvas(&output, source, h, v, event.amount_v120(Axis::Vertical));
                self.update_pointer_focus();
                return;
            }
        }

        let mut frame = AxisFrame::new(event.time_msec()).source(source);
        if h != 0.0 {
            frame = frame
                .relative_direction(Axis::Horizontal, event.relative_direction(Axis::Horizontal))
                .value(Axis::Horizontal, h);
            if let Some(d) = event.amount_v120(Axis::Horizontal) {
                frame = frame.v120(Axis::Horizontal, d as i32);
            }
        }
        if v != 0.0 {
            frame = frame
                .relative_direction(Axis::Vertical, event.relative_direction(Axis::Vertical))
                .value(Axis::Vertical, v);
            if let Some(d) = event.amount_v120(Axis::Vertical) {
                frame = frame.v120(Axis::Vertical, d as i32);
            }
        }
        if source == AxisSource::Finger {
            if event.amount(Axis::Horizontal) == Some(0.0) {
                frame = frame.stop(Axis::Horizontal);
            }
            if event.amount(Axis::Vertical) == Some(0.0) {
                frame = frame.stop(Axis::Vertical);
            }
        }
        let pointer = self.pointer.clone();
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    fn canvas_scroll_allowed(&self) -> bool {
        !self.pointer.is_grabbed()
            && !self.lock.is_locked()
            && (self.super_held() || matches!(self.under(), Under::Empty))
    }

    // ---- synthetic input (ipc `pointer …`, used by tests) -------------------

    pub fn synthetic_motion(&mut self, time: u32) {
        self.pointer_moved(time);
    }

    pub fn synthetic_button(&mut self, button: u32, state: ButtonState, time: u32) {
        self.handle_button(button, state, time);
    }

    /// Wheel notches over the canvas (positive = scroll down = zoom out).
    pub fn synthetic_wheel(&mut self, notches: f64) {
        if self.canvas_scroll_allowed() {
            if let Some(output) = self.active_output() {
                self.scroll_canvas(
                    &output,
                    AxisSource::Wheel,
                    0.0,
                    notches * 15.0,
                    Some(notches * 120.0),
                );
                self.update_pointer_focus();
            }
        }
    }

    /// Wheel zooms around the cursor; touchpad scrolling pans (Ctrl+scroll
    /// zooms); Shift+wheel pans sideways.
    fn scroll_canvas(
        &mut self,
        output: &Output,
        source: AxisSource,
        h: f64,
        v: f64,
        v120: Option<f64>,
    ) {
        let mods = self.seat.get_keyboard().map(|k| k.modifier_state());
        let (ctrl, shift) = mods.map(|m| (m.ctrl, m.shift)).unwrap_or_default();
        let anchor = self.cursor_local(output);
        let wheel = matches!(source, AxisSource::Wheel | AxisSource::WheelTilt);
        if wheel && !shift && (self.config.canvas.wheel_zooms || self.super_held()) {
            let notches = v120.map(|d| d / 120.0).unwrap_or(v / 15.0);
            let factor = self.config.canvas.zoom_step.powf(-notches);
            self.zoom_at(output, anchor, factor, true);
        } else if ctrl && !wheel {
            self.zoom_at(output, anchor, (1.0 - v * 0.01).clamp(0.5, 2.0), false);
        } else {
            let delta = if shift && wheel {
                Point::from((-v * 3.0, 0.0))
            } else {
                Point::from((-h, -v))
            };
            let factor = if wheel { 3.0 } else { 1.0 };
            self.pan(output, delta.upscale(factor), false);
        }
    }
}

fn window_under(idx: usize, hit: Hit) -> Under {
    match hit {
        Hit::Surface {
            surface,
            surface_loc,
            location,
        } => Under::Surface {
            surface,
            surface_loc,
            location,
            window: Some(idx),
            layer: None,
        },
        hit => Under::Decoration { window: idx, hit },
    }
}
