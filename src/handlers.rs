//! Wayland protocol handlers: thin glue between smithay and `State`.

use std::os::fd::OwnedFd;

use smithay::{
    backend::{allocator::dmabuf::Dmabuf, renderer::utils::on_commit_buffer_handler},
    delegate_compositor, delegate_cursor_shape, delegate_data_control, delegate_data_device,
    delegate_dmabuf, delegate_fractional_scale, delegate_idle_inhibit, delegate_idle_notify,
    delegate_keyboard_shortcuts_inhibit, delegate_layer_shell, delegate_output,
    delegate_pointer_constraints, delegate_pointer_gestures, delegate_presentation,
    delegate_primary_selection, delegate_relative_pointer, delegate_seat, delegate_session_lock,
    delegate_shm, delegate_single_pixel_buffer, delegate_viewporter, delegate_xdg_activation,
    delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{
        LayerSurface, PopupKeyboardGrab, PopupKind, PopupPointerGrab, PopupUngrabStrategy, Window,
        WindowSurfaceType, find_popup_root_surface, get_popup_toplevel_coords,
        layer_map_for_output,
    },
    input::{
        Seat, SeatHandler, SeatState,
        pointer::{CursorImageStatus, Focus, PointerHandle},
    },
    output::Output,
    reexports::{
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
            shell::server::xdg_toplevel::ResizeEdge,
        },
        wayland_server::{
            Client, Resource,
            protocol::{
                wl_buffer::WlBuffer, wl_output::WlOutput, wl_seat::WlSeat, wl_surface::WlSurface,
            },
        },
    },
    utils::{Logical, Point, Rectangle, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent,
            is_sync_subsurface, with_states,
        },
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        fractional_scale::{FractionalScaleHandler, with_fractional_scale},
        idle_inhibit::IdleInhibitHandler,
        idle_notify::{IdleNotifierHandler, IdleNotifierState},
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState,
            KeyboardShortcutsInhibitor,
        },
        output::OutputHandler,
        pointer_constraints::{PointerConstraintsHandler, with_pointer_constraint},
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                set_data_device_focus,
            },
            primary_selection::{
                PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
            },
            wlr_data_control::{DataControlHandler, DataControlState},
        },
        session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker},
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgPopupSurfaceData,
                XdgShellHandler, XdgShellState, XdgToplevelSurfaceData,
                decoration::XdgDecorationHandler,
            },
        },
        shm::{ShmHandler, ShmState},
        tablet_manager::TabletSeatHandler,
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
    },
};
use tracing::{info, warn};

use crate::{
    shell::Placement,
    state::{ClientState, LockState, State},
};

// ---- compositor --------------------------------------------------------------

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<ClientState>()
            .expect("every client is inserted with ClientState")
            .compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        crate::backend::early_import(self, surface);

        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        if !is_sync_subsurface(surface) {
            if let Some(idx) = self.index_of_surface(&root) {
                self.windows[idx].window.on_commit();
                if &root == surface {
                    self.window_committed(idx);
                }
            }
        }

        self.popups.commit(surface);
        self.ensure_initial_configure(surface);
        match self.commit_targets(&root) {
            Some(outputs) => outputs.iter().for_each(|o| self.queue_redraw(o)),
            None => self.queue_redraw_all(),
        }
    }
}

impl State {
    /// Outputs that show `root` (a window or a layer surface), so a commit
    /// only repaints those. `None`: not known, repaint everything.
    fn commit_targets(&self, root: &WlSurface) -> Option<Vec<Output>> {
        if let Some(idx) = self.index_of_surface(root) {
            let m = &self.windows[idx];
            if !m.mapped {
                return None;
            }
            let shown: Vec<Output> = self
                .outputs
                .iter()
                .filter(|o| self.view(m, o).is_some())
                .cloned()
                .collect();
            // Hidden windows still need the slow frame tick of the first output.
            return Some(if shown.is_empty() {
                self.outputs.first().cloned().into_iter().collect()
            } else {
                shown
            });
        }
        self.outputs
            .iter()
            .find(|o| {
                layer_map_for_output(o)
                    .layer_for_surface(root, WindowSurfaceType::TOPLEVEL)
                    .is_some()
            })
            .map(|o| vec![o.clone()])
    }

    fn window_committed(&mut self, idx: usize) {
        let Some(surface) = self.windows[idx].wl_surface() else {
            return;
        };
        let has_buffer =
            smithay::backend::renderer::utils::with_renderer_surface_state(&surface, |s| {
                s.buffer().is_some()
            })
            .unwrap_or(false);
        let m = &mut self.windows[idx];
        if !m.mapped && has_buffer {
            self.map_window(idx);
            return;
        }
        if m.mapped && !has_buffer {
            m.mapped = false;
            let window = m.window.clone();
            if self.focused.as_ref() == Some(&window) {
                self.focused = None;
                self.refresh_keyboard_focus();
            }
            return;
        }
        let idx = if m.late_rules.is_some() {
            // Rules may refocus and so reorder the window list.
            let id = m.id;
            self.late_rules(idx);
            match self.index_of_id(id) {
                Some(i) => i,
                None => return,
            }
        } else {
            idx
        };
        let m = &mut self.windows[idx];
        if let Some(anchor) = m.anchor {
            let size = m.window.geometry().size.to_f64();
            if matches!(
                anchor.edges,
                ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft
            ) {
                m.pos.x = anchor.right - size.w;
            }
            if matches!(
                anchor.edges,
                ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight
            ) {
                m.pos.y = anchor.bottom - size.h;
            }
        }
    }

    fn ensure_initial_configure(&mut self, surface: &WlSurface) {
        if let Some(idx) = self.index_of_surface(surface) {
            if let Some(toplevel) = self.windows[idx].window.toplevel().cloned() {
                let sent = with_states(surface, |states| {
                    states
                        .data_map
                        .get::<XdgToplevelSurfaceData>()
                        .is_some_and(|d| d.lock().is_ok_and(|d| d.initial_configure_sent))
                });
                if !sent {
                    self.claim_area(idx);
                    if let Some(size) = self.initial_size(idx) {
                        toplevel.with_pending_state(|s| s.size = Some(size));
                    }
                    toplevel.send_configure();
                }
            }
            return;
        }

        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface) {
            let sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgPopupSurfaceData>()
                    .is_some_and(|d| d.lock().is_ok_and(|d| d.initial_configure_sent))
            });
            if !sent {
                if let Err(err) = popup.send_configure() {
                    warn!("popup configure failed: {err}");
                }
            }
            return;
        }

        let layer_output = self.outputs.iter().find(|o| {
            layer_map_for_output(o)
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .is_some()
        });
        if let Some(output) = layer_output.cloned() {
            let sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .is_some_and(|d| d.lock().is_ok_and(|d| d.initial_configure_sent))
            });
            let mut map = layer_map_for_output(&output);
            map.arrange();
            if !sent {
                if let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) {
                    layer.layer_surface().send_configure();
                }
            }
            drop(map);
            self.refresh_keyboard_focus();
        }
    }

    /// Area a popup may occupy, relative to its root's coordinate origin.
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        let popup_offset = get_popup_toplevel_coords(&kind).to_f64();

        let target: Option<Rectangle<f64, Logical>> =
            if let Some(idx) = self.index_of_surface(&root) {
                let m = &self.windows[idx];
                self.outputs.iter().find_map(|o| {
                    let view = self.view(m, o)?;
                    let size = self.output_geometry(o).size.to_f64();
                    let tl = view.to_local(Point::from((0.0, 0.0)));
                    let geo = m.window.geometry().loc.to_f64();
                    Some(Rectangle::new(
                        tl + geo - popup_offset,
                        size.downscale(view.scale),
                    ))
                })
            } else {
                self.outputs.iter().find_map(|o| {
                    let map = layer_map_for_output(o);
                    let layer = map.layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)?;
                    let geo = map.layer_geometry(layer)?;
                    let size = self.output_geometry(o).size.to_f64();
                    Some(Rectangle::new(
                        (geo.loc.to_f64().upscale(-1.0)) - popup_offset,
                        size,
                    ))
                })
            };
        if let Some(target) = target {
            popup.with_pending_state(|state| {
                state.geometry = state
                    .positioner
                    .get_unconstrained_geometry(target.to_i32_round());
            });
        }
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self
            .dmabuf_state
            .as_mut()
            .expect("dmabuf global exists before clients connect")
            .0
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        if crate::backend::import_dmabuf(self, &dmabuf) {
            let _ = notifier.successful::<State>();
        } else {
            notifier.failed();
        }
    }
}

// ---- xdg shell ---------------------------------------------------------------

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // Decoration mode is only set once the client binds xdg-decoration;
        // apps that never do (GTK4) draw their own titlebar.
        self.add_window(Window::new_wayland_window(surface));
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            warn!("failed to track popup: {err}");
        }
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn grab(&mut self, surface: PopupSurface, seat: WlSeat, serial: Serial) {
        let Some(seat) = Seat::<State>::from_resource(&seat) else {
            return;
        };
        let kind = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        let Ok(mut grab) = self.popups.grab_popup(root, kind, &seat, serial) else {
            return;
        };

        if let Some(keyboard) = seat.get_keyboard() {
            if keyboard.is_grabbed()
                && !(keyboard.has_grab(serial)
                    || keyboard.has_grab(grab.previous_serial().unwrap_or(serial)))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            keyboard.set_focus(self, grab.current_grab(), serial);
            keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
        }
        if let Some(pointer) = seat.get_pointer() {
            if pointer.is_grabbed()
                && !(pointer.has_grab(serial)
                    || pointer.has_grab(grab.previous_serial().unwrap_or_else(|| grab.serial())))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
        }
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: WlSeat, serial: Serial) {
        if !self.client_grab_valid(&seat, surface.wl_surface(), serial) {
            return;
        }
        if let Some(idx) = self.index_of_surface(surface.wl_surface()) {
            crate::grabs::start_move(self, idx, serial);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: WlSeat,
        serial: Serial,
        edges: ResizeEdge,
    ) {
        if !self.client_grab_valid(&seat, surface.wl_surface(), serial) {
            return;
        }
        if let Some(idx) = self.index_of_surface(surface.wl_surface()) {
            crate::grabs::start_resize(self, idx, edges, serial);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        match self.index_of_surface(surface.wl_surface()) {
            Some(idx) if self.windows[idx].mapped => self.maximize(idx),
            // Before mapping: just acknowledge, we place it normally.
            _ => {
                if surface.is_initial_configure_sent() {
                    surface.send_configure();
                }
            }
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(idx) = self.index_of_surface(surface.wl_surface()) {
            if self.windows[idx].placement == Placement::Maximized {
                self.restore(idx);
            }
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<WlOutput>) {
        match self.index_of_surface(surface.wl_surface()) {
            Some(idx) if self.windows[idx].mapped => {
                if self.windows[idx].placement != Placement::Fullscreen {
                    self.fullscreen(idx);
                }
            }
            _ => {
                if surface.is_initial_configure_sent() {
                    surface.send_configure();
                }
            }
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(idx) = self.index_of_surface(surface.wl_surface()) {
            if self.windows[idx].placement == Placement::Fullscreen {
                self.restore(idx);
            }
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(idx) = self.index_of_surface(surface.wl_surface()) {
            let window = self.windows[idx].window.clone();
            self.remove_window(&window);
        }
    }

    fn popup_destroyed(&mut self, _surface: PopupSurface) {
        self.queue_redraw_all();
    }
}

impl State {
    /// A client may start a move/resize only from its own active click.
    fn client_grab_valid(&self, seat: &WlSeat, surface: &WlSurface, serial: Serial) -> bool {
        let Some(seat) = Seat::<State>::from_resource(seat) else {
            return false;
        };
        let Some(pointer) = seat.get_pointer() else {
            return false;
        };
        if !pointer.has_grab(serial) {
            return false;
        }
        pointer
            .grab_start_data()
            .and_then(|d| d.focus)
            .is_some_and(|(focus, _)| focus.id().same_client_as(&surface.id()))
    }
}

impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        let mode = if self.config.general.prefer_server_decorations {
            DecorationMode::ServerSide
        } else {
            DecorationMode::ClientSide
        };
        toplevel.with_pending_state(|s| s.decoration_mode = Some(mode));
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: DecorationMode) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(mode));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        self.new_decoration(toplevel.clone());
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
}

// ---- layer shell (bars, launchers, wallpapers, notifications) ---------------

impl WlrLayerShellHandler for State {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.active_output());
        let Some(output) = output else {
            surface.send_close();
            return;
        };
        info!("layer surface `{namespace}` on {}", output.name());
        let mut map = layer_map_for_output(&output);
        if let Err(err) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            warn!("failed to map layer surface: {err}");
        }
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        for output in self.outputs.clone() {
            let mut map = layer_map_for_output(&output);
            let layer = map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();
            if let Some(layer) = layer {
                map.unmap_layer(&layer);
                if self.layer_focus.as_ref() == Some(layer.wl_surface()) {
                    self.layer_focus = None;
                }
            }
        }
        self.refresh_keyboard_focus();
        self.queue_redraw_all();
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        self.unconstrain_popup(&popup);
    }
}

// ---- seat, selection ---------------------------------------------------------

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<State> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client.clone());
        set_primary_focus(dh, seat, client);
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
        self.queue_redraw_all();
    }
}

impl TabletSeatHandler for State {}

impl SelectionHandler for State {
    type SelectionUserData = ();
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for State {}

impl ServerDndGrabHandler for State {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}

impl DataControlHandler for State {
    fn data_control_state(&self) -> &DataControlState {
        &self.data_control_state
    }
}

impl OutputHandler for State {}

// ---- scaling, activation -----------------------------------------------------

impl FractionalScaleHandler for State {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self
            .active_output()
            .map(|o| o.current_scale().fractional_scale())
            .unwrap_or(1.0);
        with_states(&surface, |states| {
            with_fractional_scale(states, |fs| fs.set_preferred_scale(scale));
        });
    }
}

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        _token: XdgActivationToken,
        data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        if data.timestamp.elapsed().as_secs() >= 10 {
            return;
        }
        if let Some(idx) = self.index_of_surface(&surface) {
            let window = self.windows[idx].window.clone();
            let ws = self.windows[idx].ws;
            if self.windows[idx].placement != Placement::Pinned {
                self.switch_workspace(ws);
            }
            self.focus_window(Some(window));
            self.reveal(0);
        }
    }
}

// ---- lock screen, idle -------------------------------------------------------

impl SessionLockHandler for State {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        info!("session locked");
        // Nothing but the lock surface (or black) is drawn from now on, so
        // the lock can be confirmed right away.
        confirmation.lock();
        self.lock = LockState::Locked;
        self.interaction = None;
        self.refresh_keyboard_focus();
        self.queue_redraw_all();
    }

    fn unlock(&mut self) {
        info!("session unlocked");
        self.lock = LockState::Unlocked;
        self.lock_surfaces.clear();
        self.refresh_keyboard_focus();
        self.queue_redraw_all();
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else {
            return;
        };
        let size = self.output_geometry(&output).size;
        surface.with_pending_state(|s| s.size = Some((size.w as u32, size.h as u32).into()));
        surface.send_configure();
        self.lock_surfaces.insert(output.name(), surface);
        self.refresh_keyboard_focus();
        self.queue_redraw_all();
    }
}

impl IdleNotifierHandler for State {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.idle_notifier_state
    }
}

impl IdleInhibitHandler for State {
    fn inhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.push(surface);
        self.idle_notifier_state.set_is_inhibited(true);
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.retain(|s| s != &surface);
        let inhibited = !self.idle_inhibitors.is_empty();
        self.idle_notifier_state.set_is_inhibited(inhibited);
    }
}

impl KeyboardShortcutsInhibitHandler for State {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.keyboard_shortcuts_inhibit_state
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        inhibitor.activate();
    }
}

impl PointerConstraintsHandler for State {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        if pointer.current_focus().as_ref() == Some(surface) {
            with_pointer_constraint(surface, pointer, |c| {
                if let Some(c) = c {
                    c.activate();
                }
            });
        }
    }

    fn cursor_position_hint(
        &mut self,
        _surface: &WlSurface,
        _pointer: &PointerHandle<Self>,
        _location: Point<f64, Logical>,
    ) {
    }
}

delegate_compositor!(State);
delegate_shm!(State);
delegate_dmabuf!(State);
delegate_xdg_shell!(State);
delegate_xdg_decoration!(State);
delegate_layer_shell!(State);
delegate_seat!(State);
delegate_data_device!(State);
delegate_primary_selection!(State);
delegate_data_control!(State);
delegate_output!(State);
delegate_viewporter!(State);
delegate_fractional_scale!(State);
delegate_presentation!(State);
delegate_xdg_activation!(State);
delegate_session_lock!(State);
delegate_idle_notify!(State);
delegate_idle_inhibit!(State);
delegate_keyboard_shortcuts_inhibit!(State);
delegate_pointer_constraints!(State);
delegate_relative_pointer!(State);
delegate_pointer_gestures!(State);
delegate_cursor_shape!(State);
delegate_single_pixel_buffer!(State);
