//! Compositor state: protocol globals, windows, workspaces and the seat.

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::OsString,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use smithay::{
    desktop::{PopupManager, Window, layer_map_for_output},
    input::{
        Seat, SeatState,
        keyboard::XkbConfig,
        pointer::{CursorImageStatus, PointerHandle},
    },
    output::Output,
    reexports::{
        calloop::{Interest, LoopHandle, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Rectangle, SERIAL_COUNTER},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        cursor_shape::CursorShapeManagerState,
        dmabuf::{DmabufGlobal, DmabufState},
        fractional_scale::FractionalScaleManagerState,
        idle_inhibit::IdleInhibitManagerState,
        idle_notify::IdleNotifierState,
        keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitState,
        output::OutputManagerState,
        pointer_constraints::PointerConstraintsState,
        pointer_gestures::PointerGesturesState,
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
        selection::{
            data_device::DataDeviceState, primary_selection::PrimarySelectionState,
            wlr_data_control::DataControlState,
        },
        session_lock::{LockSurface, SessionLockManagerState},
        shell::{
            wlr_layer::WlrLayerShellState,
            xdg::{XdgShellState, decoration::XdgDecorationState},
        },
        shm::ShmState,
        single_pixel_buffer::SinglePixelBufferState,
        socket::ListeningSocketSource,
        viewporter::ViewporterState,
        xdg_activation::XdgActivationState,
    },
};
use tracing::{info, warn};

use crate::{
    backend::Backend,
    canvas::{Camera, WORKSPACES, Workspace},
    config::Config,
    cursor::CursorTheme,
    grabs::Interaction,
    render::RenderCache,
    shell::Managed,
};

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// Per-output data, stored in the output's user data.
#[derive(Debug, Default)]
pub struct OutputState {
    /// Workspace shown on this output.
    pub ws: usize,
    pub redraw: RedrawState,
    /// A redraw was requested while the backend was busy rendering.
    pub redraw_pending: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RedrawState {
    #[default]
    Idle,
    /// A render is scheduled on the event loop.
    Queued,
    /// A frame is on screen-flip; `queued` asks for another one afterwards.
    WaitingForVblank { queued: bool },
}

pub fn output_state(output: &Output) -> std::cell::RefMut<'_, OutputState> {
    output
        .user_data()
        .insert_if_missing(|| RefCell::new(OutputState::default()));
    output
        .user_data()
        .get::<RefCell<OutputState>>()
        .expect("inserted above")
        .borrow_mut()
}

#[derive(Default)]
pub enum LockState {
    #[default]
    Unlocked,
    Locked,
}

impl LockState {
    pub fn is_locked(&self) -> bool {
        !matches!(self, LockState::Unlocked)
    }
}

// Several protocol states are only held so their globals stay alive.
#[allow(dead_code)]
pub struct State {
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, State>,
    pub loop_signal: LoopSignal,
    pub clock: Clock<Monotonic>,
    pub socket_name: OsString,
    pub config: Config,
    pub backend: Backend,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    pub dmabuf_state: Option<(DmabufState, DmabufGlobal)>,
    pub seat_state: SeatState<State>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub data_control_state: DataControlState,
    pub output_manager_state: OutputManagerState,
    pub viewporter_state: ViewporterState,
    pub fractional_scale_state: FractionalScaleManagerState,
    pub presentation_state: PresentationState,
    pub xdg_activation_state: XdgActivationState,
    pub session_lock_state: SessionLockManagerState,
    pub idle_notifier_state: IdleNotifierState<State>,
    pub idle_inhibit_state: IdleInhibitManagerState,
    pub keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    pub pointer_constraints_state: PointerConstraintsState,
    pub relative_pointer_state: RelativePointerManagerState,
    pub pointer_gestures_state: PointerGesturesState,
    pub cursor_shape_state: CursorShapeManagerState,
    pub single_pixel_buffer_state: SinglePixelBufferState,

    pub popups: PopupManager,
    pub seat: Seat<State>,
    pub pointer: PointerHandle<State>,
    /// Windows, topmost first.
    pub windows: Vec<Managed>,
    pub next_window_id: u64,
    pub workspaces: [Workspace; WORKSPACES],
    pub outputs: Vec<Output>,
    pub focused: Option<Window>,
    /// Keyboard focus held by a layer surface (launcher, shell popup).
    pub layer_focus: Option<WlSurface>,

    /// Pointer position in global layout coordinates.
    pub cursor: Point<f64, Logical>,
    pub cursor_status: CursorImageStatus,
    /// Cursor shown over compositor UI (titlebars, edges, drags).
    pub cursor_override: Option<smithay::input::pointer::CursorIcon>,
    pub cursor_theme: CursorTheme,
    pub interaction: Option<Interaction>,
    pub hover: Option<crate::shell::Hover>,
    pub last_title_click: Option<(u64, Instant)>,
    pub snap_preview: Option<(Output, Rectangle<f64, Logical>)>,
    /// The minimap called up at the cursor (`position = "cursor"`).
    pub map_popup: Option<crate::minimap::MapPopup>,
    pub swipe: Option<Point<f64, Logical>>,
    /// Middle press over a window, not yet known to be a click or a pan.
    pub pending_middle: Option<(Point<f64, Logical>, u32)>,
    /// Key presses consumed by bindings; their releases are swallowed too.
    pub suppressed_keys: Vec<smithay::backend::input::Keycode>,
    /// Camera to return to when the overview is toggled off.
    pub pre_overview: Option<(usize, Camera)>,
    pub pinch: Option<f64>,
    /// When the last canvas scroll happened; scrolling keeps moving the
    /// canvas until a short pause even if a window slides under the cursor.
    pub scroll_latch: Option<Instant>,

    pub lock: LockState,
    pub lock_surfaces: HashMap<String, LockSurface>,
    pub idle_inhibitors: Vec<WlSurface>,
    pub render_cache: RenderCache,
    /// Off while rendering a screenshot that asked not to include the cursor.
    pub render_cursor: bool,
    pub pending_copies: Vec<crate::screencopy::PendingCopy>,
    pub bar: crate::bar::BarFeed,
    pub events: crate::events::EventFeed,
    /// Window lists for taskbars (foreign-toplevel protocols).
    pub toplevels: crate::toplevels::Toplevels,
    pub protocols: crate::protocols::Protocols,
    /// Monitor settings applied at runtime (wlr-output-management, e.g.
    /// kanshi or wdisplays); they win over `[[output]]` until a reload.
    pub output_overrides: Vec<crate::config::OutputConfig>,
    pub gamma: crate::gamma::GammaControls,
    pub output_management: crate::output_management::OutputManagement,
    /// Outputs showing a copy of another output (`mirror` in the config).
    /// They are not in `outputs`: no workspace, bar or cursor of their own.
    pub mirrors: Vec<Mirror>,
    /// Each mirror's own render cache, so drawing the source at another
    /// scale does not make its cached elements look changed on every frame.
    pub mirror_caches: HashMap<String, RenderCache>,
    pub profile_pending: crate::profiles::PendingPlacements,
    /// Camera history and bookmarks.
    pub jumps: crate::jumps::Jumps,
    /// Groups whose windows are put away, only their frame showing.
    pub collapsed_groups: std::collections::HashSet<String>,
    /// The group the last `group toggle` collapsed and the window that got
    /// focus instead, so pressing it again expands that group.
    pub last_collapsed: Option<(String, Option<u64>)>,
    /// Area picked for the next window and the selection mode, see area.rs.
    pub area: crate::area::AreaState,
    /// The cell grid of `tile`, while it is open, see tile.rs.
    pub tile: Option<crate::tile::TileMode>,
    /// Last `snap left|right`: window, side, share step, when; see tile.rs.
    pub last_snap: Option<(u64, crate::action::Dir, usize, Instant)>,
    /// Number typed so far with Super held (Super+1, 2 = window 12), see numbering.rs.
    pub window_chord: Option<(usize, Instant)>,
    /// Ideal size shown while a size is dragged, see sizehint.rs.
    pub size_hint: Option<crate::sizehint::SizeHint>,
    /// Where windows were before `ideal-size`, see tile.rs.
    pub ideal_restore: HashMap<u64, crate::tile::IdealRestore>,
    /// When `scratchpad` last started its app, so a double press starts one.
    pub scratchpad_spawned: Option<Instant>,
    /// The running `present` show, see present.rs.
    pub presentation: Option<crate::present::Show>,
    /// Drawings on the canvas and the pen, see draw.rs.
    pub draw: crate::draw::Drawing,
    /// App icons for semantic zoom.
    pub icons: crate::icons::Icons,
    pub xwayland_display: Option<String>,
    pub children: Vec<std::process::Child>,
}

#[derive(Debug, Clone)]
pub struct Mirror {
    pub output: Output,
    /// Name of the output it shows.
    pub source: String,
}

impl State {
    pub fn new(
        display: Display<State>,
        loop_handle: LoopHandle<'static, State>,
        loop_signal: LoopSignal,
        backend: Backend,
        config: Config,
    ) -> anyhow::Result<State> {
        let dh = display.handle();
        let clock = Clock::new();

        let socket = ListeningSocketSource::new_auto()?;
        let socket_name = socket.socket_name().to_os_string();
        loop_handle
            .insert_source(socket, |stream, _, state| {
                if let Err(err) = state
                    .display_handle
                    .insert_client(stream, Arc::new(ClientState::default()))
                {
                    warn!("failed to add client: {err}");
                }
            })
            .map_err(|e| anyhow::anyhow!("socket source: {e}"))?;
        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // SAFETY: the display is owned by this source and never dropped while dispatching.
                    unsafe {
                        display.get_mut().dispatch_clients(state)?;
                    }
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| anyhow::anyhow!("display source: {e}"))?;

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&dh);
        let data_control_state =
            DataControlState::new::<Self, _>(&dh, Some(&primary_selection_state), |_| true);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let viewporter_state = ViewporterState::new::<Self>(&dh);
        let fractional_scale_state = FractionalScaleManagerState::new::<Self>(&dh);
        let presentation_state = PresentationState::new::<Self>(&dh, clock.id() as u32);
        let xdg_activation_state = XdgActivationState::new::<Self>(&dh);
        let session_lock_state = SessionLockManagerState::new::<Self, _>(&dh, |_| true);
        let idle_notifier_state = IdleNotifierState::new(&dh, loop_handle.clone());
        let idle_inhibit_state = IdleInhibitManagerState::new::<Self>(&dh);
        let keyboard_shortcuts_inhibit_state = KeyboardShortcutsInhibitState::new::<Self>(&dh);
        let pointer_constraints_state = PointerConstraintsState::new::<Self>(&dh);
        let relative_pointer_state = RelativePointerManagerState::new::<Self>(&dh);
        let pointer_gestures_state = PointerGesturesState::new::<Self>(&dh);
        let cursor_shape_state = CursorShapeManagerState::new::<Self>(&dh);
        let single_pixel_buffer_state = SinglePixelBufferState::new::<Self>(&dh);
        crate::screencopy::init(&dh);
        crate::image_copy::init(&dh);
        let toplevels = crate::toplevels::Toplevels::new(&dh);
        let protocols = crate::protocols::Protocols::new(&dh);
        crate::gamma::init(&dh);
        crate::output_management::init(&dh);

        let mut seat = seat_state.new_wl_seat(&dh, backend.seat_name());
        let pointer = seat.add_pointer();
        let input = &config.input;
        let xkb = XkbConfig {
            rules: "",
            model: &input.model,
            layout: &input.layouts,
            variant: &input.variant,
            options: (!input.options.is_empty()).then(|| input.options.clone()),
        };
        if let Err(err) = seat.add_keyboard(xkb, input.repeat_delay, input.repeat_rate) {
            warn!(
                "keyboard layout `{}` failed ({err:?}), falling back to us",
                input.layouts
            );
            seat.add_keyboard(XkbConfig::default(), input.repeat_delay, input.repeat_rate)
                .map_err(|e| anyhow::anyhow!("keyboard init: {e:?}"))?;
        }

        let cursor_theme =
            CursorTheme::load(&config.general.cursor_theme, config.general.cursor_size);

        Ok(State {
            display_handle: dh,
            loop_handle,
            loop_signal,
            clock,
            socket_name,
            config,
            backend,
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            layer_shell_state,
            shm_state,
            dmabuf_state: None,
            seat_state,
            data_device_state,
            primary_selection_state,
            data_control_state,
            output_manager_state,
            viewporter_state,
            fractional_scale_state,
            presentation_state,
            xdg_activation_state,
            session_lock_state,
            idle_notifier_state,
            idle_inhibit_state,
            keyboard_shortcuts_inhibit_state,
            pointer_constraints_state,
            relative_pointer_state,
            pointer_gestures_state,
            cursor_shape_state,
            single_pixel_buffer_state,
            popups: PopupManager::default(),
            seat,
            pointer,
            windows: Vec::new(),
            next_window_id: 1,
            workspaces: [Workspace::default(); WORKSPACES],
            outputs: Vec::new(),
            focused: None,
            layer_focus: None,
            cursor: Point::from((0.0, 0.0)),
            cursor_status: CursorImageStatus::default_named(),
            cursor_override: None,
            cursor_theme,
            interaction: None,
            hover: None,
            last_title_click: None,
            snap_preview: None,
            map_popup: None,
            swipe: None,
            pending_middle: None,
            suppressed_keys: Vec::new(),
            pre_overview: None,
            pinch: None,
            scroll_latch: None,
            lock: LockState::default(),
            lock_surfaces: HashMap::new(),
            idle_inhibitors: Vec::new(),
            render_cache: RenderCache::default(),
            render_cursor: true,
            pending_copies: Vec::new(),
            bar: Default::default(),
            events: Default::default(),
            toplevels,
            protocols,
            output_overrides: Vec::new(),
            gamma: Default::default(),
            output_management: Default::default(),
            mirrors: Vec::new(),
            mirror_caches: HashMap::new(),
            profile_pending: Default::default(),
            jumps: crate::jumps::Jumps::load(),
            collapsed_groups: Default::default(),
            last_collapsed: None,
            area: Default::default(),
            tile: None,
            last_snap: None,
            window_chord: None,
            size_hint: None,
            ideal_restore: HashMap::new(),
            scratchpad_spawned: None,
            presentation: None,
            draw: crate::draw::Drawing::load(),
            icons: Default::default(),
            xwayland_display: None,
            children: Vec::new(),
        })
    }

    // ---- outputs & workspaces ----------------------------------------------

    /// Settings for a monitor: a runtime override, else `[[output]]`.
    pub fn output_config(&self, name: &str) -> crate::config::OutputConfig {
        self.output_overrides
            .iter()
            .find(|o| o.name == name)
            .or_else(|| self.config.output(name))
            .cloned()
            .unwrap_or_else(|| crate::config::OutputConfig {
                name: name.to_string(),
                ..Default::default()
            })
    }

    pub fn output_scale(&self, name: &str) -> f64 {
        self.output_config(name)
            .scale
            .unwrap_or(self.config.general.scale)
            .clamp(0.5, 4.0)
    }

    pub fn output_geometry(&self, output: &Output) -> Rectangle<i32, Logical> {
        let loc = output.current_location();
        let size = output
            .current_mode()
            .map(|m| {
                output
                    .current_transform()
                    .transform_size(m.size)
                    .to_f64()
                    .to_logical(output.current_scale().fractional_scale())
                    .to_i32_round()
            })
            .unwrap_or_default();
        Rectangle::new(loc, size)
    }

    pub fn output_at(&self, global: Point<f64, Logical>) -> Option<Output> {
        self.outputs
            .iter()
            .find(|o| self.output_geometry(o).to_f64().contains(global))
            .cloned()
    }

    /// Output under the cursor, falling back to the first one.
    pub fn active_output(&self) -> Option<Output> {
        self.output_at(self.cursor)
            .or_else(|| self.outputs.first().cloned())
    }

    pub fn output_by_name(&self, name: &str) -> Option<Output> {
        self.outputs.iter().find(|o| o.name() == name).cloned()
    }

    pub fn output_ws(&self, output: &Output) -> usize {
        output_state(output).ws
    }

    /// Area not covered by exclusive layer surfaces (the bar), output-local.
    pub fn usable_area(&self, output: &Output) -> Rectangle<i32, Logical> {
        let zone = layer_map_for_output(output).non_exclusive_zone();
        if zone.size.w > 0 && zone.size.h > 0 {
            zone
        } else {
            Rectangle::from_size(self.output_geometry(output).size)
        }
    }

    pub fn camera_slot(&self, ws: usize) -> usize {
        if self.config.canvas.per_workspace_camera {
            ws
        } else {
            0
        }
    }

    pub fn workspace(&self, ws: usize) -> &Workspace {
        &self.workspaces[self.camera_slot(ws)]
    }

    pub fn workspace_mut(&mut self, ws: usize) -> &mut Workspace {
        let slot = self.camera_slot(ws);
        &mut self.workspaces[slot]
    }

    pub fn camera(&self, output: &Output) -> Camera {
        self.workspace(self.output_ws(output))
            .camera(Instant::now())
    }

    pub fn camera_duration(&self) -> Duration {
        if self.config.animation.enabled {
            Duration::from_millis(self.config.animation.camera_ms)
        } else {
            Duration::ZERO
        }
    }

    /// Cursor position relative to `output` (screen space).
    pub fn cursor_local(&self, output: &Output) -> Point<f64, Logical> {
        self.cursor - self.output_geometry(output).loc.to_f64()
    }

    pub fn clamp_cursor(&mut self) {
        if self.outputs.is_empty() || self.output_at(self.cursor).is_some() {
            return;
        }
        let nearest = self
            .outputs
            .iter()
            .map(|o| self.output_geometry(o).to_f64())
            .map(|g| {
                let x = self.cursor.x.clamp(g.loc.x, g.loc.x + g.size.w - 1.0);
                let y = self.cursor.y.clamp(g.loc.y, g.loc.y + g.size.h - 1.0);
                Point::<f64, Logical>::from((x, y))
            })
            .min_by(|a, b| {
                let da = (a.x - self.cursor.x).powi(2) + (a.y - self.cursor.y).powi(2);
                let db = (b.x - self.cursor.x).powi(2) + (b.y - self.cursor.y).powi(2);
                da.total_cmp(&db)
            });
        if let Some(p) = nearest {
            self.cursor = p;
        }
    }

    // ---- redraw ------------------------------------------------------------

    pub fn queue_redraw(&mut self, output: &Output) {
        crate::backend::queue_redraw(self, output);
        let name = output.name();
        for mirror in self.mirrors.clone() {
            if mirror.source == name {
                crate::backend::queue_redraw(self, &mirror.output);
            }
        }
    }

    /// The output a mirror copies, when `output` is a mirror.
    pub fn mirror_source(&self, output: &Output) -> Option<Option<Output>> {
        let mirror = self.mirrors.iter().find(|m| &m.output == output)?;
        Some(self.output_by_name(&mirror.source))
    }

    pub fn queue_redraw_all(&mut self) {
        for output in self.outputs.clone() {
            self.queue_redraw(&output);
        }
    }

    /// True while something on `output` is moving on its own.
    pub fn is_animating(&mut self, output: &Output) -> bool {
        let now = Instant::now();
        let ws = self.output_ws(output);
        let camera_moving = self.workspace_mut(ws).tick(now);
        let window_anim = self.config.animation.enabled
            && self
                .windows
                .iter()
                .any(|w| w.anim_progress(now, &self.config) < 1.0);
        camera_moving || window_anim || self.map_popup_fading(now)
    }

    // ---- processes ---------------------------------------------------------

    pub fn spawn(&mut self, command: &str) {
        info!("spawn: {command}");
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(command)
            .env("WAYLAND_DISPLAY", &self.socket_name)
            .env("XDG_CURRENT_DESKTOP", "pzwm")
            .env("XDG_SESSION_DESKTOP", "pzwm")
            .env("XDG_SESSION_TYPE", "wayland")
            .env(
                "PZWM_SOCKET",
                crate::ipc::socket_path(&self.socket_name.to_string_lossy()),
            )
            .env(
                "UMBRIEL_SOCKET",
                crate::bar::socket_path(&self.socket_name.to_string_lossy()),
            )
            .stdin(Stdio::null());
        match &self.xwayland_display {
            Some(display) => cmd.env("DISPLAY", display),
            None => cmd.env_remove("DISPLAY"),
        };
        match cmd.spawn() {
            Ok(child) => {
                self.children.push(child);
                self.reap_children();
            }
            Err(err) => warn!("failed to spawn `{command}`: {err}"),
        }
    }

    pub fn reap_children(&mut self) {
        self.children
            .retain_mut(|c| matches!(c.try_wait(), Ok(None)));
    }

    pub fn next_serial(&self) -> smithay::utils::Serial {
        SERIAL_COUNTER.next_serial()
    }

    pub fn now_ms(&self) -> u32 {
        let t: Duration = self.clock.now().into();
        t.as_millis() as u32
    }
}
