//! Keyboard: key bindings (matched in every layout), the action
//! dispatcher and config reload.

use smithay::{
    backend::input::{KeyState, Keycode},
    input::keyboard::{FilterResult, Keysym, Layout},
    utils::Point,
    wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitorSeat,
};
use tracing::info;

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
            self.do_action(action);
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
            Action::FocusDir(dir) => self.focus_dir(dir),
            Action::FocusNext => self.focus_cycle(true),
            Action::FocusPrev => self.focus_cycle(false),
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
            Action::ReloadConfig => self.reload_config(),
            Action::Quit => {
                info!("quit requested");
                self.loop_signal.stop();
            }
            Action::SwitchVt(vt) => crate::backend::switch_vt(self, vt),
        }
        self.update_pointer_focus();
    }

    pub fn reload_config(&mut self) {
        let config = Config::load();
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
        self.queue_redraw_all();
        info!("config reloaded");
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
