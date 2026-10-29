//! Window management on the canvas: placement, focus, maximize/fullscreen
//! with restore, pinning, snapping, workspaces and hit testing.

use std::time::{Duration, Instant};

use smithay::{
    desktop::{Window, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::{
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
            shell::server::xdg_toplevel::{ResizeEdge, State as ToplevelState},
        },
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{IsAlive, Logical, Point, Rectangle, Size},
    wayland::{
        compositor::with_states,
        seat::WaylandFocus,
        shell::{
            wlr_layer::{KeyboardInteractivity, Layer, LayerSurfaceCachedState},
            xdg::{SurfaceCachedState, XdgToplevelSurfaceData},
        },
    },
};

use crate::{canvas::ease_out, config::Config, state::State};

/// `share` (clamped to 0.1 .. 1.0) of `len`, kept within the client's
/// `min`/`max` size; a `max` of 0 means no limit.
pub fn share_of(len: f64, share: f64, min: i32, max: i32) -> i32 {
    let want = ((len * share.clamp(0.1, 1.0)).round() as i32)
        .max(min)
        .max(1);
    if max > 0 { want.min(max) } else { want }
}

pub struct CanvasGeometry {
    pub ws: usize,
    pub pos: Point<f64, Logical>,
    pub size: Size<i32, Logical>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Lives on the canvas, pans and zooms with it.
    Canvas,
    /// Fixed on screen (output-local position) above the canvas.
    Pinned,
    /// Fills the usable area of an output.
    Maximized,
    /// Covers a whole output, above the bar.
    Fullscreen,
}

impl Placement {
    /// Stacking class: higher draws above lower regardless of focus order.
    fn rank(self) -> u8 {
        match self {
            Placement::Canvas => 0,
            Placement::Maximized => 1,
            Placement::Pinned => 2,
            Placement::Fullscreen => 3,
        }
    }
}

/// Where a maximized/fullscreen window returns to.
#[derive(Debug, Clone)]
pub struct Restore {
    placement: Placement,
    pos: Point<f64, Logical>,
    size: Size<i32, Logical>,
    output: Option<String>,
    ws: usize,
}

/// Keeps the edge opposite to the dragged one in place while the client
/// catches up with a resize (positions are window units).
#[derive(Debug, Clone, Copy)]
pub struct ResizeAnchor {
    pub edges: ResizeEdge,
    pub right: f64,
    pub bottom: f64,
}

pub struct Managed {
    pub id: u64,
    pub window: Window,
    pub ws: usize,
    /// Canvas: top-left of the content in canvas units.
    /// Pinned: top-left of the content in output-local screen units.
    pub pos: Point<f64, Logical>,
    pub placement: Placement,
    pub output: Option<String>,
    pub restore: Option<Restore>,
    pub mapped: bool,
    pub opened_at: Option<Instant>,
    pub anchor: Option<ResizeAnchor>,
    /// The scratchpad window, see scratchpad.rs.
    pub scratchpad: bool,
    /// Put away: the scratchpad while it is not called up.
    pub hidden: bool,
    /// Group the window belongs to, see groups.rs.
    pub group: Option<String>,
    /// Area picked for it before it opened (workspace, canvas frame), see area.rs.
    pub area: Option<(usize, Rectangle<f64, Logical>)>,
    /// Until then, window rules are checked again on every commit: apps
    /// such as Chrome set their title only after the window appears.
    pub late_rules: Option<Instant>,
    /// "pick" placement: hidden until its place is dragged out, see area.rs.
    pub awaiting_pick: bool,
}

impl Managed {
    /// 0..1 progress of the open animation (1 when done or disabled).
    pub fn anim_progress(&self, now: Instant, config: &Config) -> f64 {
        match self.opened_at {
            Some(start) if config.animation.enabled && config.animation.window_ms > 0 => {
                let total = Duration::from_millis(config.animation.window_ms).as_secs_f64();
                (now.saturating_duration_since(start).as_secs_f64() / total).min(1.0)
            }
            _ => 1.0,
        }
    }

    pub fn wl_surface(&self) -> Option<WlSurface> {
        self.window.wl_surface().map(|s| s.into_owned())
    }

    pub fn size(&self) -> Size<f64, Logical> {
        self.window.geometry().size.to_f64()
    }
}

/// How a window maps onto an output: content top-left (output-local screen
/// coordinates) and screen pixels per window pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub origin: Point<f64, Logical>,
    pub scale: f64,
}

impl View {
    pub fn to_local(self, screen: Point<f64, Logical>) -> Point<f64, Logical> {
        (screen - self.origin).downscale(self.scale)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleButton {
    Close,
    Maximize,
    Pin,
}

impl TitleButton {
    pub fn index(self) -> f32 {
        match self {
            TitleButton::Close => 1.0,
            TitleButton::Maximize => 2.0,
            TitleButton::Pin => 3.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hover {
    pub id: u64,
    pub button: TitleButton,
}

#[derive(Debug, Clone)]
pub enum Hit {
    /// A client surface; `surface_loc` and `location` share the window's
    /// local coordinate space (relative to the content top-left).
    Surface {
        surface: WlSurface,
        surface_loc: Point<f64, Logical>,
        location: Point<f64, Logical>,
    },
    Title,
    Button(TitleButton),
    Edge(ResizeEdge),
}

/// Screen-space margin around windows that grabs resize.
const CASCADE_STEP: f64 = 32.0;
/// How long rules wait for a title or app id that arrives late.
const LATE_RULES: Duration = Duration::from_secs(5);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Centers of the three titlebar buttons in window-local coordinates
/// (content top-left at 0,0; the titlebar spans y in [-title, 0)).
/// Must match the layout in the frame shader.
pub fn button_center(width: f64, title: f64, button: TitleButton) -> Point<f64, Logical> {
    let i = button.index() as f64 - 1.0;
    Point::from((width - title * 0.62 - i * title * 0.72, -title * 0.5))
}

impl State {
    pub fn index_of(&self, window: &Window) -> Option<usize> {
        self.windows.iter().position(|m| &m.window == window)
    }

    pub fn index_of_surface(&self, surface: &WlSurface) -> Option<usize> {
        self.windows
            .iter()
            .position(|m| m.window.wl_surface().as_deref() == Some(surface))
    }

    pub fn index_of_id(&self, id: u64) -> Option<usize> {
        self.windows.iter().position(|m| m.id == id)
    }

    pub fn focused_index(&self) -> Option<usize> {
        self.focused.as_ref().and_then(|w| self.index_of(w))
    }

    pub fn is_ssd(window: &Window) -> bool {
        window
            .toplevel()
            .is_some_and(|t| t.current_state().decoration_mode == Some(DecorationMode::ServerSide))
    }

    /// Titlebar height in window units (0 without one).
    pub fn title_height(&self, m: &Managed) -> f64 {
        let has_title = self.config.appearance.titlebar
            && m.placement != Placement::Fullscreen
            && Self::is_ssd(&m.window);
        if has_title {
            self.config.appearance.titlebar_height as f64
        } else {
            0.0
        }
    }

    fn window_output(&self, m: &Managed) -> Option<Output> {
        m.output.as_deref().and_then(|n| self.output_by_name(n))
    }

    /// Mapped and not put away (scratchpad, collapsed group).
    pub fn shown(&self, m: &Managed) -> bool {
        m.mapped
            && !m.hidden
            && !m.awaiting_pick
            && !m
                .group
                .as_ref()
                .is_some_and(|g| self.collapsed_groups.contains(g))
    }

    pub fn view(&self, m: &Managed, output: &Output) -> Option<View> {
        if !self.shown(m) {
            return None;
        }
        let on_output = || m.output.as_deref() == Some(output.name().as_str());
        match m.placement {
            Placement::Canvas => {
                if m.ws != self.output_ws(output) {
                    return None;
                }
                let cam = self.camera(output);
                Some(View {
                    origin: cam.to_screen(m.pos),
                    scale: cam.zoom,
                })
            }
            Placement::Pinned => on_output().then_some(View {
                origin: m.pos,
                scale: 1.0,
            }),
            Placement::Maximized => {
                if !on_output() || m.ws != self.output_ws(output) {
                    return None;
                }
                let area = self.usable_area(output).to_f64();
                let gap = self.config.appearance.gaps as f64;
                let title = self.title_height(m);
                Some(View {
                    origin: area.loc + Point::from((gap, gap + title)),
                    scale: 1.0,
                })
            }
            Placement::Fullscreen => {
                (on_output() && m.ws == self.output_ws(output)).then_some(View {
                    origin: Point::from((0.0, 0.0)),
                    scale: 1.0,
                })
            }
        }
    }

    /// Frame (titlebar + content) in output-local screen coordinates.
    pub fn frame_rect(&self, m: &Managed, view: &View) -> Rectangle<f64, Logical> {
        let title = self.title_height(m);
        let size = m.size();
        Rectangle::new(
            Point::from((view.origin.x, view.origin.y - title * view.scale)),
            Size::from((size.w * view.scale, (size.h + title) * view.scale)),
        )
    }

    /// Frame in canvas coordinates for canvas windows.
    pub fn canvas_frame(&self, m: &Managed) -> Rectangle<f64, Logical> {
        let title = self.title_height(m);
        let size = m.size();
        Rectangle::new(
            Point::from((m.pos.x, m.pos.y - title)),
            Size::from((size.w, size.h + title)),
        )
    }

    /// Indices of windows visible on `output`, topmost first.
    pub fn stacking(&self, output: &Output) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.windows.len())
            .filter(|&i| self.view(&self.windows[i], output).is_some())
            .collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.windows[i].placement.rank()));
        order
    }

    /// What is under `local` (output-local) among windows, topmost first.
    pub fn window_hit(&self, output: &Output, local: Point<f64, Logical>) -> Option<(usize, Hit)> {
        for i in self.stacking(output) {
            let m = &self.windows[i];
            let Some(view) = self.view(m, output) else {
                continue;
            };
            if let Some(hit) = self.hit_window(m, &view, local) {
                return Some((i, hit));
            }
        }
        None
    }

    fn hit_window(&self, m: &Managed, view: &View, screen: Point<f64, Logical>) -> Option<Hit> {
        let p = view.to_local(screen);
        let geo = m.window.geometry();
        let size = geo.size.to_f64();
        let title = self.title_height(m);

        // Popups and the surface itself first: popups may extend outside.
        let surface_point = p + geo.loc.to_f64();
        if let Some((surface, loc)) = m
            .window
            .surface_under(surface_point, WindowSurfaceType::ALL)
        {
            return Some(Hit::Surface {
                surface,
                surface_loc: (loc - geo.loc).to_f64(),
                location: p,
            });
        }

        let inside_x = p.x >= 0.0 && p.x < size.w;
        if title > 0.0 && inside_x && p.y >= -title && p.y < 0.0 {
            let r = title * 0.32;
            for button in [TitleButton::Close, TitleButton::Maximize, TitleButton::Pin] {
                let c = button_center(size.w, title, button);
                if (p.x - c.x).powi(2) + (p.y - c.y).powi(2) <= r * r {
                    return Some(Hit::Button(button));
                }
            }
            return Some(Hit::Title);
        }

        if matches!(m.placement, Placement::Maximized | Placement::Fullscreen) {
            return None;
        }
        let margin = self.config.canvas.resize_margin.max(1.0) / view.scale;
        let corner = self.config.canvas.resize_corner.max(0.0) / view.scale;
        let (left, right) = (
            p.x < 0.0 && p.x >= -margin,
            p.x >= size.w && p.x < size.w + margin,
        );
        let (top, bottom) = (
            p.y < -title && p.y >= -title - margin,
            p.y >= size.h && p.y < size.h + margin,
        );
        let in_x = p.x >= -margin && p.x < size.w + margin;
        let in_y = p.y >= -title - margin && p.y < size.h + margin;
        if !in_x || !in_y {
            return None;
        }
        // Near a corner the side zones resize diagonally, so corners are
        // easy to catch without pixel hunting.
        let side_x = left || right;
        let side_y = top || bottom;
        let west = left || (side_y && p.x < corner);
        let east = right || (side_y && p.x >= size.w - corner);
        let north = top || (side_x && p.y < -title + corner);
        let south = bottom || (side_x && p.y >= size.h - corner);
        let edge = match (west, east, north, south) {
            (true, _, true, _) => ResizeEdge::TopLeft,
            (true, _, _, true) => ResizeEdge::BottomLeft,
            (_, true, true, _) => ResizeEdge::TopRight,
            (_, true, _, true) => ResizeEdge::BottomRight,
            (true, ..) => ResizeEdge::Left,
            (_, true, ..) => ResizeEdge::Right,
            (_, _, true, _) => ResizeEdge::Top,
            (_, _, _, true) => ResizeEdge::Bottom,
            _ => return None,
        };
        Some(Hit::Edge(edge))
    }

    // ---- mapping -----------------------------------------------------------

    pub fn add_window(&mut self, window: Window) {
        let output = self.active_output();
        let ws = output.as_ref().map(|o| self.output_ws(o)).unwrap_or(0);
        if let (Some(toplevel), Some(output)) = (window.toplevel(), output.as_ref()) {
            let area = self.usable_area(output);
            toplevel.with_pending_state(|s| s.bounds = Some(area.size));
        }
        let id = self.next_window_id;
        self.next_window_id += 1;
        self.windows.insert(
            0,
            Managed {
                id,
                window,
                ws,
                pos: Point::from((0.0, 0.0)),
                placement: Placement::Canvas,
                output: None,
                restore: None,
                mapped: false,
                opened_at: None,
                anchor: None,
                scratchpad: false,
                hidden: false,
                group: None,
                area: None,
                late_rules: None,
                awaiting_pick: false,
            },
        );
    }

    pub fn app_id(&self, m: &Managed) -> Option<String> {
        let toplevel = m.window.toplevel()?;
        with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok().and_then(|d| d.app_id.clone()))
        })
    }

    pub fn title(&self, m: &Managed) -> Option<String> {
        let toplevel = m.window.toplevel()?;
        with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok().and_then(|d| d.title.clone()))
        })
    }

    /// Where a window sits on the canvas; for a maximized one, the place it
    /// returns to.
    pub fn canvas_geometry(&self, idx: usize) -> Option<CanvasGeometry> {
        let m = &self.windows[idx];
        match m.placement {
            Placement::Canvas => Some(CanvasGeometry {
                ws: m.ws,
                pos: m.pos,
                size: m.window.geometry().size,
            }),
            Placement::Maximized => {
                let r = m.restore.as_ref()?;
                Some(CanvasGeometry {
                    ws: r.ws,
                    pos: r.pos,
                    size: r.size,
                })
            }
            Placement::Pinned | Placement::Fullscreen => None,
        }
    }

    /// Size a new window is asked to open at, so every app starts the same
    /// instead of at whatever it remembered. `None` leaves the choice to the
    /// app: dialogs and fixed-size windows.
    pub fn initial_size(&self, idx: usize) -> Option<Size<i32, Logical>> {
        let m = &self.windows[idx];
        let toplevel = m.window.toplevel()?;
        if toplevel.parent().is_some() {
            return None;
        }
        let (min, max) = with_states(toplevel.wl_surface(), |states| {
            let mut cached = states.cached_state.get::<SurfaceCachedState>();
            let current = cached.current();
            (current.min_size, current.max_size)
        });
        if max.w > 0 && max.h > 0 && min == max {
            return None;
        }
        if let Some((_, rect)) = m.area {
            let title = self.title_height(m);
            let clamp = |v: f64, lo: i32, hi: i32| {
                let v = (v.round() as i32).max(lo).max(1);
                if hi > 0 { v.min(hi) } else { v }
            };
            return Some(Size::from((
                clamp(rect.size.w, min.w, max.w),
                clamp(rect.size.h - title, min.h, max.h),
            )));
        }
        if let Some(size) = self.app_id(m).and_then(|a| self.profile_size(&a)) {
            return Some(size);
        }
        let area = self.usable_area(&self.active_output()?).size.to_f64();
        let title = self.title_height(m);
        let (share_w, share_h) = match self.rule_effect(m) {
            Some(rule) if rule.size.is_some() => {
                let size = rule.size?;
                let w = size.w.max(min.w);
                let h = size.h.max(min.h);
                return Some(Size::from((
                    if max.w > 0 { w.min(max.w) } else { w },
                    if max.h > 0 { h.min(max.h) } else { h },
                )));
            }
            Some(rule) if rule.scratchpad => {
                (self.config.scratchpad.width, self.config.scratchpad.height)
            }
            _ => (
                self.config.canvas.new_window_width,
                self.config.canvas.new_window_height,
            ),
        };
        Some(Size::from((
            share_of(area.w, share_w, min.w, max.w),
            share_of(area.h - title, share_h, min.h, max.h),
        )))
    }

    /// First buffer arrived: choose a spot in view and focus the window.
    pub fn map_window(&mut self, idx: usize) {
        let Some(output) = self.active_output() else {
            self.windows[idx].mapped = true;
            return;
        };
        let ws = self.output_ws(&output);
        let cam = self.workspace(ws).target();
        let area = self.usable_area(&output).to_f64();
        let visible = cam.visible(area);
        let parent_frame = self.parent_frame(idx, ws);

        let m = &self.windows[idx];
        let size = m.size();
        let title = self.title_height(m);
        let target = parent_frame.unwrap_or(visible);
        let mut pos = Point::<f64, Logical>::from((
            target.loc.x + (target.size.w - size.w) / 2.0,
            target.loc.y + (target.size.h - size.h - title) / 2.0 + title,
        ));
        let area = m.area;
        let sibling = match (parent_frame, area) {
            (None, None) => self.sibling_spot(idx, ws),
            _ => None,
        };
        if let Some((_, rect)) = area {
            pos = Point::from((rect.loc.x, rect.loc.y + title));
        } else if let Some(spot) = sibling {
            pos = spot;
        } else if parent_frame.is_none() {
            while self.windows.iter().any(|o| {
                o.mapped && o.ws == ws && o.placement == Placement::Canvas && {
                    let d = o.pos - pos;
                    d.x.abs() < 4.0 && d.y.abs() < 4.0
                }
            }) {
                pos += Point::from((CASCADE_STEP, CASCADE_STEP));
            }
        }
        let m = &mut self.windows[idx];
        let id = m.id;
        m.ws = area.map_or(ws, |(area_ws, _)| area_ws);
        m.pos = pos;
        m.mapped = true;
        m.opened_at = Some(Instant::now());
        let from_profile = self.place_from_profile(idx);
        let ruled = !from_profile && self.apply_rules(idx);
        // Dialogs and an app's further windows open by their parent or
        // sibling; everything else asks for its place in "pick" mode.
        let pick = !ruled
            && !from_profile
            && area.is_none()
            && parent_frame.is_none()
            && sibling.is_none()
            && self.config.canvas.new_window_placement == "pick"
            && !self.windows[idx].scratchpad
            && !self.is_dialog_like(idx);
        if !ruled {
            if !from_profile {
                self.windows[idx].late_rules = Some(Instant::now() + LATE_RULES);
            }
            if pick {
                self.wait_for_pick(idx);
            } else {
                let window = self.windows[idx].window.clone();
                self.focus_window(Some(window));
            }
        }
        if let Some(idx) = self.index_of_id(id) {
            let m = &mut self.windows[idx];
            if let Some((area_ws, rect)) = area {
                // The picked area wins over rules that also place the window.
                m.ws = area_ws;
                m.pos = pos;
                m.area = None;
                // The first configure went out before the title bar was
                // known: fit the content under it now.
                let content = Size::from((
                    rect.size.w.round() as i32,
                    (rect.size.h - title).round().max(1.0) as i32,
                ));
                if (m.size().h - content.h as f64).abs() > 1.0 {
                    self.configure(idx, Some(content), false, false);
                }
            } else if sibling.is_some() && !ruled && !from_profile {
                self.reveal(idx);
            }
        }
        self.queue_redraw_all();
    }

    /// The window opened without a matching rule: its title or app id may
    /// have come later (Chrome), so try the rules again, once, until the
    /// user moves it or `LATE_RULES` passes.
    pub fn late_rules(&mut self, idx: usize) {
        let Some(until) = self.windows[idx].late_rules else {
            return;
        };
        if Instant::now() >= until {
            self.windows[idx].late_rules = None;
            return;
        }
        let Some(effect) = self.rule_effect(&self.windows[idx]) else {
            return;
        };
        self.windows[idx].late_rules = None;
        // The rule places it: no need to ask for a place any more.
        if std::mem::take(&mut self.windows[idx].awaiting_pick) {
            let id = self.windows[idx].id;
            self.forget_pick(id);
        }
        if let Some(size) = effect.size {
            // We are inside the window's commit: resize once it is handled.
            let id = self.windows[idx].id;
            self.loop_handle.insert_idle(move |state| {
                if let Some(i) = state.index_of_id(id) {
                    state.configure(i, Some(size), false, false);
                }
            });
        }
        self.apply_rules(idx);
        self.queue_redraw_all();
    }

    fn parent_frame(&self, idx: usize, ws: usize) -> Option<Rectangle<f64, Logical>> {
        let parent = self.windows[idx].window.toplevel()?.parent()?;
        let p = self.windows.iter().find(|m| {
            m.window.wl_surface().as_deref() == Some(&parent)
                && m.ws == ws
                && m.placement == Placement::Canvas
        })?;
        Some(self.canvas_frame(p))
    }

    pub fn remove_window(&mut self, window: &Window) {
        let Some(idx) = self.index_of(window) else {
            return;
        };
        let removed = self.windows.remove(idx);
        self.forget_pick(removed.id);
        if let Some(
            crate::grabs::Interaction::Move { id, .. }
            | crate::grabs::Interaction::Resize { id, .. },
        ) = &self.interaction
        {
            if *id == removed.id {
                self.interaction = None;
                self.snap_preview = None;
            }
        }
        self.render_cache.forget(removed.id);
        for cache in self.mirror_caches.values_mut() {
            cache.forget(removed.id);
        }
        if self.focused.as_ref() == Some(window) {
            self.focused = None;
            let ws = self
                .active_output()
                .map(|o| self.output_ws(&o))
                .unwrap_or(0);
            let next = self
                .windows
                .iter()
                .find(|m| self.shown(m) && (m.ws == ws || m.placement == Placement::Pinned))
                .map(|m| m.window.clone());
            self.focus_window(next);
        }
        self.queue_redraw_all();
    }

    // ---- focus -------------------------------------------------------------

    pub fn focus_window(&mut self, window: Option<Window>) {
        if let Some(w) = &window {
            if let Some(idx) = self.index_of(w) {
                let m = self.windows.remove(idx);
                self.windows.insert(0, m);
            }
        }
        for m in &self.windows {
            let active = Some(&m.window) == window.as_ref();
            if m.window.set_activated(active) {
                if let Some(t) = m.window.toplevel() {
                    if t.is_initial_configure_sent() {
                        t.send_pending_configure();
                    }
                }
            }
        }
        self.focused = window;
        self.layer_focus = None;
        self.refresh_keyboard_focus();
        self.queue_redraw_all();
    }

    /// Picks the keyboard target: lock screen, exclusive layer, clicked layer,
    /// then the focused window.
    pub fn refresh_keyboard_focus(&mut self) {
        let target = self.keyboard_target();
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        if keyboard.current_focus() != target {
            let serial = self.next_serial();
            keyboard.set_focus(self, target, serial);
        }
    }

    fn keyboard_target(&mut self) -> Option<WlSurface> {
        if self.lock.is_locked() {
            let output = self.active_output()?;
            return self
                .lock_surfaces
                .get(&output.name())
                .or_else(|| self.lock_surfaces.values().next())
                .map(|s| s.wl_surface().clone());
        }
        for output in &self.outputs {
            let map = layer_map_for_output(output);
            for layer in map.layers().rev() {
                let exclusive = with_states(layer.wl_surface(), |states| {
                    let mut cached = states.cached_state.get::<LayerSurfaceCachedState>();
                    let data = cached.current();
                    data.keyboard_interactivity == KeyboardInteractivity::Exclusive
                        && matches!(data.layer, Layer::Top | Layer::Overlay)
                });
                if exclusive {
                    return Some(layer.wl_surface().clone());
                }
            }
        }
        if let Some(surface) = self.layer_focus.clone().filter(|s| s.alive()) {
            return Some(surface);
        }
        self.focused
            .as_ref()
            .filter(|w| w.alive())
            .and_then(|w| w.wl_surface().map(|s| s.into_owned()))
    }

    // ---- window commands ---------------------------------------------------

    pub fn close_focused(&mut self) {
        if let Some(toplevel) = self.focused.as_ref().and_then(|w| w.toplevel()) {
            toplevel.send_close();
        }
    }

    pub(crate) fn configure(
        &self,
        idx: usize,
        size: Option<Size<i32, Logical>>,
        max: bool,
        full: bool,
    ) {
        let m = &self.windows[idx];
        let Some(toplevel) = m.window.toplevel() else {
            return;
        };
        toplevel.with_pending_state(|s| {
            s.size = size;
            if max {
                s.states.set(ToplevelState::Maximized)
            } else {
                s.states.unset(ToplevelState::Maximized)
            };
            if full {
                s.states.set(ToplevelState::Fullscreen)
            } else {
                s.states.unset(ToplevelState::Fullscreen)
            };
            if !full {
                s.fullscreen_output = None;
            }
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn save_restore(&mut self, idx: usize) {
        let m = &self.windows[idx];
        if m.restore.is_some() {
            return;
        }
        let size = m.window.geometry().size;
        self.windows[idx].restore = Some(Restore {
            placement: m.placement,
            pos: m.pos,
            size,
            output: m.output.clone(),
            ws: m.ws,
        });
    }

    /// Output a window currently shows on, preferring the active one.
    pub(crate) fn output_of(&self, idx: usize) -> Option<Output> {
        let m = &self.windows[idx];
        if let Some(o) = self.window_output(m) {
            return Some(o);
        }
        let active = self.active_output();
        if let Some(a) = active.as_ref().filter(|a| self.view(m, a).is_some()) {
            return Some(a.clone());
        }
        self.outputs
            .iter()
            .find(|o| self.view(m, o).is_some())
            .cloned()
            .or(active)
    }

    pub fn maximize(&mut self, idx: usize) {
        let Some(output) = self.output_of(idx) else {
            return;
        };
        self.save_restore(idx);
        let area = self.usable_area(&output);
        let gap = self.config.appearance.gaps;
        let ws = self.output_ws(&output);
        let m = &mut self.windows[idx];
        m.placement = Placement::Maximized;
        m.output = Some(output.name());
        m.ws = ws;
        m.opened_at = Some(Instant::now());
        let title = self.title_height(&self.windows[idx]) as i32;
        let size = Size::from((
            (area.size.w - 2 * gap).max(1),
            (area.size.h - 2 * gap - title).max(1),
        ));
        self.configure(idx, Some(size), true, false);
        self.queue_redraw_all();
    }

    pub fn fullscreen(&mut self, idx: usize) {
        let Some(output) = self.output_of(idx) else {
            return;
        };
        self.save_restore(idx);
        let size = self.output_geometry(&output).size;
        let ws = self.output_ws(&output);
        let m = &mut self.windows[idx];
        m.placement = Placement::Fullscreen;
        m.output = Some(output.name());
        m.ws = ws;
        self.configure(idx, Some(size), false, true);
        let window = self.windows[idx].window.clone();
        self.focus_window(Some(window));
    }

    /// Returns a maximized/fullscreen window to exactly where it was.
    pub fn restore(&mut self, idx: usize) {
        let Some(r) = self.windows[idx].restore.take() else {
            self.configure(idx, None, false, false);
            return;
        };
        let m = &mut self.windows[idx];
        m.placement = r.placement;
        m.pos = r.pos;
        m.output = r.output;
        m.ws = r.ws;
        m.opened_at = Some(Instant::now());
        self.configure(idx, Some(r.size), false, false);
        self.queue_redraw_all();
    }

    pub fn toggle_maximize(&mut self, idx: usize) {
        match self.windows[idx].placement {
            Placement::Maximized | Placement::Fullscreen => self.restore(idx),
            _ => self.maximize(idx),
        }
    }

    pub fn toggle_fullscreen(&mut self, idx: usize) {
        match self.windows[idx].placement {
            Placement::Fullscreen => self.restore(idx),
            Placement::Maximized => {
                // Keep the original restore point, just go further.
                self.windows[idx].placement = Placement::Canvas;
                self.fullscreen(idx);
            }
            _ => self.fullscreen(idx),
        }
    }

    pub fn toggle_pin(&mut self, idx: usize) {
        let Some(output) = self.output_of(idx) else {
            return;
        };
        let Some(view) = self.view(&self.windows[idx], &output) else {
            return;
        };
        match self.windows[idx].placement {
            Placement::Canvas => {
                let m = &mut self.windows[idx];
                m.placement = Placement::Pinned;
                m.pos = view.origin;
                m.output = Some(output.name());
            }
            Placement::Pinned => {
                let cam = self.camera(&output);
                let ws = self.output_ws(&output);
                let m = &mut self.windows[idx];
                m.placement = Placement::Canvas;
                m.pos = cam.to_canvas(view.origin);
                m.output = None;
                m.ws = ws;
            }
            Placement::Maximized | Placement::Fullscreen => {}
        }
        self.queue_redraw_all();
    }

    /// Moves the camera so the window sits in the middle of the screen.
    pub fn move_by(&mut self, idx: usize, delta: Point<f64, Logical>) {
        let m = &mut self.windows[idx];
        if matches!(m.placement, Placement::Canvas | Placement::Pinned) {
            m.pos += delta;
            self.queue_redraw_all();
        }
    }

    pub fn resize_by(&mut self, idx: usize, dw: i32, dh: i32) {
        let m = &self.windows[idx];
        if !matches!(m.placement, Placement::Canvas | Placement::Pinned) {
            return;
        }
        let size = m.window.geometry().size;
        let new = Size::from(((size.w + dw).max(80), (size.h + dh).max(60)));
        self.configure(idx, Some(new), false, false);
    }

    // ---- titlebar ----------------------------------------------------------

    /// Click on a titlebar: returns true when it was a double-click.
    pub fn title_clicked(&mut self, id: u64) -> bool {
        let now = Instant::now();
        let double = matches!(self.last_title_click, Some((last, t)) if last == id && now - t < DOUBLE_CLICK);
        self.last_title_click = if double { None } else { Some((id, now)) };
        double
    }

    pub fn press_button(&mut self, idx: usize, button: TitleButton) {
        match button {
            TitleButton::Close => {
                if let Some(t) = self.windows[idx].window.toplevel() {
                    t.send_close();
                }
            }
            TitleButton::Maximize => self.toggle_maximize(idx),
            TitleButton::Pin => self.toggle_pin(idx),
        }
    }

    /// Bring outputs, surfaces and focus in line after outputs changed.
    pub fn relayout_outputs(&mut self) {
        for m in &mut self.windows {
            if let Some(name) = m.output.clone() {
                if !self.outputs.iter().any(|o| o.name() == name) {
                    m.output = self.outputs.first().map(|o| o.name());
                }
            }
        }
        for output in self.outputs.clone() {
            layer_map_for_output(&output).arrange();
        }
        self.clamp_cursor();
        self.queue_redraw_all();
    }

    /// Smooth open scale/alpha for a window (scale factor, alpha).
    pub fn open_effect(&self, m: &Managed, now: Instant) -> (f64, f32) {
        let t = ease_out(m.anim_progress(now, &self.config));
        (0.94 + 0.06 * t, (0.15 + 0.85 * t) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::share_of;

    #[test]
    fn share_of_takes_fraction_of_screen() {
        assert_eq!(share_of(1000.0, 0.6, 0, 0), 600);
    }

    #[test]
    fn share_of_respects_client_limits() {
        assert_eq!(share_of(1000.0, 0.6, 800, 0), 800);
        assert_eq!(share_of(1000.0, 0.6, 0, 400), 400);
    }

    #[test]
    fn share_of_clamps_bad_fractions() {
        assert_eq!(share_of(1000.0, 5.0, 0, 0), 1000);
        assert_eq!(share_of(1000.0, 0.0, 0, 0), 100);
    }
}
