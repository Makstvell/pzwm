//! Keyboard: key bindings (matched in every layout), the action
//! dispatcher and config reload.

use smithay::{
    backend::input::{KeyState, Keycode},
    desktop::layer_map_for_output,
    input::keyboard::{FilterResult, Keysym, Layout},
    output::Scale,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::Point,
    wayland::{
        compositor::with_states, fractional_scale::with_fractional_scale,
        keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitorSeat,
    },
};
use std::time::{Duration, SystemTime};

use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use tracing::{info, warn};

use crate::{
    action::{Action, Dir},
    config::{Config, Mods},
    state::State,
};

impl State {
    // ---- keyboard ----------------------------------------------------------

    pub fn handle_key(&mut self, keycode: Keycode, key_state: KeyState, time: u32) {
        self.notify_activity();
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = self.next_serial();
        let inhibited = self.shortcuts_inhibited();
        let locked = self.lock.is_locked();

        let action = keyboard.input::<Option<Action>, _>(
            self,
            keycode,
            key_state,
            serial,
            time,
            |state, mods, handle| {
                if key_state == KeyState::Released {
                    if state.map_popup.as_ref().and_then(|p| p.key) == Some(keycode) {
                        state.map_key_released();
                    }
                    return match state.suppressed_keys.iter().position(|k| *k == keycode) {
                        Some(i) => {
                            state.suppressed_keys.remove(i);
                            FilterResult::Intercept(None)
                        }
                        None => FilterResult::Forward,
                    };
                }
                // Ctrl+Alt+Fn always works, even on the lock screen.
                let vt = handle.modified_syms().iter().find_map(|sym| {
                    let raw = sym.raw();
                    (Keysym::XF86_Switch_VT_1.raw()..=Keysym::XF86_Switch_VT_12.raw())
                        .contains(&raw)
                        .then(|| (raw - Keysym::XF86_Switch_VT_1.raw() + 1) as i32)
                });
                if let Some(vt) = vt {
                    state.suppressed_keys.push(keycode);
                    return FilterResult::Intercept(Some(Action::SwitchVt(vt)));
                }
                if locked || inhibited {
                    return FilterResult::Forward;
                }
                let pressed = Mods::from_state(mods);
                let sym = handle
                    .raw_latin_sym_or_raw_current_sym()
                    .unwrap_or_else(|| handle.modified_sym());
                let sym = lowercase(sym);
                if state.map_popup.is_some() && sym == Keysym::Escape {
                    state.suppressed_keys.push(keycode);
                    return FilterResult::Intercept(Some(Action::ToggleMinimap));
                }
                match find_bind(&state.config, pressed, sym) {
                    Some(action) => {
                        state.suppressed_keys.push(keycode);
                        FilterResult::Intercept(Some(action))
                    }
                    None => FilterResult::Forward,
                }
            },
        );
        if let Some(Some(action)) = action {
            let calls_map = action == Action::ToggleMinimap;
            self.do_action(action);
            // Remember the key so letting go of it after a hold closes the map.
            if let Some(popup) = self.map_popup.as_mut().filter(|_| calls_map) {
                popup.key.get_or_insert(keycode);
            }
        }
    }

    fn shortcuts_inhibited(&self) -> bool {
        self.focused
            .as_ref()
            .and_then(|w| w.toplevel())
            .and_then(|t| {
                self.seat
                    .keyboard_shortcuts_inhibitor_for_surface(t.wl_surface())
            })
            .is_some_and(|i| i.is_active())
    }

    pub fn do_action(&mut self, action: Action) {
        let focused = self.focused_index();
        let output = self.active_output();
        let step = self.config.canvas.window_step;
        match action {
            Action::Spawn(cmd) => self.spawn(&cmd),
            Action::Close => self.close_focused(),
            Action::ToggleMaximize => focused.into_iter().for_each(|i| self.toggle_maximize(i)),
            Action::ToggleFullscreen => focused.into_iter().for_each(|i| self.toggle_fullscreen(i)),
            Action::TogglePin => focused.into_iter().for_each(|i| self.toggle_pin(i)),
            Action::CenterWindow => focused.into_iter().for_each(|i| self.center_on(i)),
            Action::FitWindow => focused.into_iter().for_each(|i| self.fit_window(i)),
            Action::Profile(args) => {
                let args: Vec<&str> = args.split_whitespace().collect();
                let (title, body) = match self.profile_command(&args) {
                    Ok(msg) => ("pzwm", msg),
                    Err(err) => ("pzwm: profile error", err),
                };
                self.spawn(&format!(
                    "notify-send -a pzwm {} {}",
                    shell_quote(title),
                    shell_quote(&body)
                ));
            }
            Action::FocusDir(dir) => self.focus_dir(dir),
            Action::FocusNext => self.focus_cycle(true),
            Action::FocusPrev => self.focus_cycle(false),
            Action::FocusNth(n) => self.focus_nth(n),
            Action::MoveWindow(dir) => {
                if let Some(i) = focused {
                    let (dx, dy) = dir.vector();
                    let scale = output
                        .as_ref()
                        .and_then(|o| self.view(&self.windows[i], o))
                        .map(|v| v.scale)
                        .unwrap_or(1.0);
                    self.move_by(i, Point::from((dx, dy)).upscale(step as f64 / scale));
                    self.reveal(i);
                }
            }
            Action::ResizeWindow(dir) => {
                if let Some(i) = focused {
                    let (dw, dh) = match dir {
                        Dir::Left => (-step, 0),
                        Dir::Right => (step, 0),
                        Dir::Up => (0, -step),
                        Dir::Down => (0, step),
                    };
                    self.resize_by(i, dw, dh);
                }
            }
            Action::Snap(dir) => focused.into_iter().for_each(|i| self.snap(i, dir)),
            Action::Pan(dir) => {
                if let Some(o) = output {
                    let (dx, dy) = dir.vector();
                    // Keys move the view, so the canvas slides the other way.
                    let delta = Point::from((-dx, -dy)).upscale(self.config.canvas.pan_step);
                    self.pan(&o, delta, true);
                }
            }
            Action::ZoomIn | Action::ZoomOut => {
                if let Some(o) = output {
                    let area = self.usable_area(&o).to_f64();
                    let center = area.loc + area.size.downscale(2.0).to_point();
                    let step = self.config.canvas.zoom_step.powi(2);
                    let factor = if action == Action::ZoomIn {
                        step
                    } else {
                        1.0 / step
                    };
                    self.zoom_at(&o, center, factor, true);
                }
            }
            Action::ZoomReset => output.into_iter().for_each(|o| self.zoom_reset(&o)),
            Action::Overview => output.into_iter().for_each(|o| self.overview(&o)),
            Action::Home => output.into_iter().for_each(|o| self.home(&o)),
            Action::ToggleMinimap => self.toggle_minimap(),
            Action::Workspace(ws) => self.switch_workspace(ws),
            Action::MoveToWorkspace(ws) => focused
                .into_iter()
                .for_each(|i| self.move_to_workspace(i, ws)),
            Action::NextLayout => {
                if let Some(keyboard) = self.seat.get_keyboard() {
                    keyboard.with_xkb_state(self, |mut ctx| ctx.cycle_next_layout());
                }
            }
            Action::SetLayout(n) => {
                if let Some(keyboard) = self.seat.get_keyboard() {
                    keyboard.with_xkb_state(self, |mut ctx| ctx.set_layout(Layout(n)));
                }
            }
            Action::ReloadConfig => {
                let _ = self.reload_config();
            }
            Action::Quit => {
                info!("quit requested");
                self.loop_signal.stop();
            }
            Action::SwitchVt(vt) => crate::backend::switch_vt(self, vt),
        }
        self.update_pointer_focus();
    }

    /// Applies the config file to the running session. A file that does not
    /// parse is reported (log + notification) and the current config stays.
    pub fn reload_config(&mut self) -> Result<(), String> {
        let config = match Config::try_load() {
            Ok(config) => config,
            Err(err) => {
                warn!("config not reloaded: {err}");
                self.spawn(&format!(
                    "notify-send -a pzwm 'pzwm: config error' {}",
                    shell_quote(&err)
                ));
                return Err(err);
            }
        };
        if let Some(keyboard) = self.seat.get_keyboard() {
            let input = &config.input;
            let xkb = smithay::input::keyboard::XkbConfig {
                rules: "",
                model: &input.model,
                layout: &input.layouts,
                variant: &input.variant,
                options: (!input.options.is_empty()).then(|| input.options.clone()),
            };
            if let Err(err) = keyboard.set_xkb_config(self, xkb) {
                tracing::warn!("keyboard layout `{}` rejected: {err:?}", input.layouts);
            }
            keyboard.change_repeat_info(input.repeat_rate, input.repeat_delay);
        }
        self.cursor_theme = crate::cursor::CursorTheme::load(
            &config.general.cursor_theme,
            config.general.cursor_size,
        );
        self.config = config;
        crate::backend::reconfigure_input(self);
        self.apply_output_scales();
        self.queue_redraw_all();
        info!("config reloaded");
        Ok(())
    }

    /// Polls the config file and reloads it when it changes (if
    /// `auto_reload` is on). Polling needs no extra dependency and copes with
    /// editors that save by replacing the file.
    pub fn watch_config(&mut self) {
        const POLL: Duration = Duration::from_millis(750);
        let mtime = || {
            std::fs::metadata(Config::path())
                .and_then(|m| m.modified())
                .ok()
        };
        let mut last: Option<SystemTime> = mtime();
        let source =
            self.loop_handle
                .insert_source(Timer::from_duration(POLL), move |_, _, state| {
                    let now = mtime();
                    if now != last {
                        last = now;
                        if state.config.general.auto_reload {
                            let _ = state.reload_config();
                        }
                    }
                    TimeoutAction::ToDuration(POLL)
                });
        if let Err(err) = source {
            warn!("config watcher: {err}");
        }
    }

    /// Applies changed `scale` / `position` settings to running outputs and
    /// tells clients their new preferred scale.
    fn apply_output_scales(&mut self) {
        let mut changed = false;
        for output in self.outputs.clone() {
            let name = output.name();
            let scale = self.config.output_scale(&name);
            let position = self
                .config
                .output(&name)
                .and_then(|o| o.position())
                .map(Point::from);
            let scale_changed = (output.current_scale().fractional_scale() - scale).abs() > 1e-6;
            let moved = position.is_some_and(|p| p != output.current_location());
            if !scale_changed && !moved {
                continue;
            }
            output.change_current_state(
                None,
                None,
                scale_changed.then_some(Scale::Fractional(scale)),
                position,
            );
            info!("output {name}: scale {scale}");
            changed = true;
        }
        if !changed {
            return;
        }
        let mut surfaces: Vec<(WlSurface, f64)> = Vec::new();
        for m in &self.windows {
            let scale = m
                .output
                .as_deref()
                .and_then(|n| self.output_by_name(n))
                .or_else(|| self.outputs.first().cloned())
                .map(|o| o.current_scale().fractional_scale())
                .unwrap_or(1.0);
            if let Some(surface) = m.wl_surface() {
                surfaces.push((surface, scale));
            }
        }
        for output in &self.outputs {
            let scale = output.current_scale().fractional_scale();
            for layer in layer_map_for_output(output).layers() {
                surfaces.push((layer.wl_surface().clone(), scale));
            }
        }
        for (surface, scale) in surfaces {
            with_states(&surface, |states| {
                with_fractional_scale(states, |fs| fs.set_preferred_scale(scale));
            });
        }
        self.relayout_outputs();
    }
}

/// Quotes `s` as one `sh` word.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn lowercase(sym: Keysym) -> Keysym {
    match sym.raw() {
        raw @ 0x41..=0x5a => Keysym::from(raw + 0x20),
        _ => sym,
    }
}

fn find_bind(config: &Config, mods: Mods, sym: Keysym) -> Option<Action> {
    config
        .keybinds
        .iter()
        .find(|k| k.mods == mods && k.sym == sym)
        .map(|k| k.action.clone())
}
