// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

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
                    // Letting go of Super ends a multi-digit window number.
                    if matches!(handle.modified_sym(), Keysym::Super_L | Keysym::Super_R) {
                        state.window_chord = None;
                    }
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
                if state.tile.is_some() && !pressed.logo {
                    if let Some(action) = tile_key(pressed, sym) {
                        state.suppressed_keys.push(keycode);
                        return FilterResult::Intercept(Some(action));
                    }
                }
                if state.area.selecting && sym == Keysym::Escape {
                    state.suppressed_keys.push(keycode);
                    return FilterResult::Intercept(Some(Action::Area("cancel".into())));
                }
                if state.area.selecting
                    && (sym == Keysym::Return || sym == Keysym::KP_Enter)
                    && !pressed.logo
                {
                    state.suppressed_keys.push(keycode);
                    return FilterResult::Intercept(Some(Action::Area("beside".into())));
                }
                if state.draw.active && !pressed.logo {
                    if let Some(action) = pen_key(pressed, sym) {
                        state.suppressed_keys.push(keycode);
                        return FilterResult::Intercept(Some(action));
                    }
                }
                match find_bind(&state.config, pressed, sym) {
                    // A digit bound to a window number may be one of several.
                    Some(Action::FocusNth(_))
                        if (Keysym::_0.raw()..=Keysym::_9.raw()).contains(&sym.raw()) =>
                    {
                        state.suppressed_keys.push(keycode);
                        let digit = (sym.raw() - Keysym::_0.raw()) as usize;
                        FilterResult::Intercept(Some(Action::WindowDigit(digit)))
                    }
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
            Action::Spawn(cmd) => {
                self.note_user_launch();
                self.spawn(&cmd)
            }
            Action::Close => self.close_focused(),
            Action::ToggleMaximize => focused.into_iter().for_each(|i| self.toggle_maximize(i)),
            Action::ToggleFullscreen => focused.into_iter().for_each(|i| self.toggle_fullscreen(i)),
            Action::TogglePin => focused.into_iter().for_each(|i| self.toggle_pin(i)),
            Action::CenterWindow => focused.into_iter().for_each(|i| {
                self.remember_camera_of_ws(self.windows[i].ws);
                self.center_on(i)
            }),
            Action::FitWindow => focused.into_iter().for_each(|i| {
                self.remember_camera_of_ws(self.windows[i].ws);
                self.fit_window(i)
            }),
            Action::Profile(args) => {
                let args: Vec<&str> = args.split_whitespace().collect();
                let result = self.profile_command(&args);
                self.notify_result("profile", result, true);
            }
            Action::Bookmark(args) => {
                let args: Vec<&str> = args.split_whitespace().collect();
                let result = self.bookmark_command(&args);
                // Saving gets a confirmation; jumping speaks for itself.
                self.notify_result("bookmark", result, args.first() != Some(&"go"));
            }
            Action::Group(args) => {
                let args: Vec<&str> = args.split_whitespace().collect();
                let result = self.group_command(&args);
                self.notify_result("group", result, args.first() == Some(&"add"));
            }
            Action::CameraBack | Action::CameraForward => {
                if let Some(o) = output {
                    self.camera_history(&o, action == Action::CameraBack);
                }
            }
            Action::Scratchpad => self.toggle_scratchpad(),
            Action::Find => self.find(),
            Action::Draw(args) => {
                let args: Vec<&str> = args.split_whitespace().collect();
                let result = self.draw_command(&args);
                self.notify_result("draw", result, false);
            }
            Action::Area(args) => {
                let result = self.area_command(&args);
                self.notify_result("area", result, false);
            }
            Action::Dpms(arg) => {
                let result = self.dpms_command(&arg);
                self.notify_result("dpms", result, false);
            }
            Action::Lock => self.lock_session(),
            Action::IdealSize => {
                if let Some(i) = focused {
                    self.ideal_size(i);
                }
            }
            Action::Tile(args) => {
                let result = self.tile_command(&args);
                self.notify_result("tile", result, false);
            }
            Action::Arrange(how) => {
                let result = self.arrange(how);
                self.notify_result("arrange", result, false);
            }
            Action::Present(args) => {
                let args: Vec<&str> = args.split_whitespace().collect();
                let result = self.present_command(&args);
                self.notify_result("present", result, args.is_empty() || args == ["start"]);
            }
            Action::FocusDir(dir) => self.focus_dir(dir),
            Action::FocusNext => self.focus_cycle(true),
            Action::FocusPrev => self.focus_cycle(false),
            Action::FocusNth(n) => {
                if let Some(o) = &output {
                    self.remember_camera(o);
                }
                self.focus_nth(n)
            }
            Action::WindowDigit(digit) => {
                if let Some(o) = &output {
                    self.remember_camera(o);
                }
                self.window_digit(digit)
            }
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
            Action::ZoomReset => output.into_iter().for_each(|o| {
                self.remember_camera(&o);
                self.zoom_reset(&o)
            }),
            Action::Overview => output.into_iter().for_each(|o| {
                self.remember_camera(&o);
                self.overview(&o)
            }),
            Action::Home => output.into_iter().for_each(|o| {
                self.remember_camera(&o);
                self.home(&o)
            }),
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

    /// Shows the outcome of a keyboard command as a notification: errors
    /// always, success only when `confirm` is set.
    fn notify_result(&mut self, what: &str, result: Result<String, String>, confirm: bool) {
        let (title, body) = match result {
            Ok(msg) if confirm => ("pzwm".to_string(), msg),
            Ok(_) => return,
            Err(err) => (format!("pzwm: {what} error"), err),
        };
        self.spawn(&format!(
            "notify-send -a pzwm {} {}",
            shell_quote(&title),
            shell_quote(&body)
        ));
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
        // The file is the truth again: runtime monitor changes are dropped.
        self.output_overrides.clear();
        crate::backend::reapply_outputs(self);
        self.apply_color_scheme();
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
    pub(crate) fn apply_output_scales(&mut self) {
        let mut changed = false;
        for output in self.outputs.clone() {
            let name = output.name();
            let scale = self.output_scale(&name);
            let position = self.output_config(&name).position().map(Point::from);
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
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Keys the pen takes while drawing is on (without Super, which still
/// reaches the bindings): Escape, Ctrl+Z, Delete and 1 … 9.
fn pen_key(mods: Mods, sym: Keysym) -> Option<Action> {
    let draw = |arg: &str| Some(Action::Draw(arg.to_string()));
    match sym {
        Keysym::Escape => draw("off"),
        Keysym::z if mods.ctrl => draw("undo"),
        Keysym::Delete => draw("clear"),
        _ if mods.ctrl || mods.alt => None,
        _ => {
            let raw = sym.raw();
            (Keysym::_1.raw()..=Keysym::_9.raw())
                .contains(&raw)
                .then(|| Action::Draw(format!("color {}", raw - Keysym::_0.raw())))
        }
    }
}

/// Keys the `tile` grid takes while it is open (without Super): arrows
/// move the block, Shift+arrows grow it, Enter applies, Escape closes.
fn tile_key(mods: Mods, sym: Keysym) -> Option<Action> {
    let tile = |arg: &str| Some(Action::Tile(arg.to_string()));
    let dir = match sym {
        Keysym::Left | Keysym::h => "left",
        Keysym::Right | Keysym::l => "right",
        Keysym::Up | Keysym::k => "up",
        Keysym::Down | Keysym::j => "down",
        Keysym::Return | Keysym::KP_Enter | Keysym::space => return tile("apply"),
        Keysym::Escape => return tile("cancel"),
        _ => return None,
    };
    if mods.shift {
        tile(&format!("grow {dir}"))
    } else {
        tile(dir)
    }
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
