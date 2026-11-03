// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Moving around the canvas with the keyboard.
//! - A `pan` bind (`Super+Alt` + arrows or h j k l) glides the view while
//!   the key is held, speeding up a little; Shift goes faster.
//! - `navigate` (`Super+N`) is a mode where the plain arrows and h j k l do
//!   the same, `+`/`-` zoom, `0` goes home, Escape or Enter leave it.

use std::time::{Duration, Instant};

use smithay::{
    backend::input::Keycode,
    input::keyboard::Keysym,
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::Point,
};

use crate::{action::Dir, state::State};

/// Time between two steps of a held pan.
const TICK: Duration = Duration::from_millis(8);
/// A held pan starts at this share of its speed and reaches full speed
/// after `RAMP`.
const START_SHARE: f64 = 0.35;
const RAMP: Duration = Duration::from_millis(350);
/// Shift held: this much faster.
const FAST: f64 = 3.0;

#[derive(Default)]
pub struct KeyPan {
    /// Keys held down that pan, with their direction.
    held: Vec<(Keycode, Dir)>,
    /// When the first of them went down.
    since: Option<Instant>,
    last_tick: Option<Instant>,
    ticking: bool,
    /// The `navigate` mode is on.
    pub mode: bool,
}

/// What a key does in the `navigate` mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKey {
    Pan(Dir),
    ZoomIn,
    ZoomOut,
    Home,
    Exit,
}

pub fn nav_key(sym: Keysym) -> Option<NavKey> {
    Some(match sym {
        Keysym::Left | Keysym::h | Keysym::H => NavKey::Pan(Dir::Left),
        Keysym::Down | Keysym::j | Keysym::J => NavKey::Pan(Dir::Down),
        Keysym::Up | Keysym::k | Keysym::K => NavKey::Pan(Dir::Up),
        Keysym::Right | Keysym::l | Keysym::L => NavKey::Pan(Dir::Right),
        Keysym::plus | Keysym::equal | Keysym::KP_Add | Keysym::i => NavKey::ZoomIn,
        Keysym::minus | Keysym::KP_Subtract | Keysym::o => NavKey::ZoomOut,
        Keysym::_0 | Keysym::Home => NavKey::Home,
        Keysym::Escape | Keysym::Return | Keysym::KP_Enter | Keysym::q => NavKey::Exit,
        _ => return None,
    })
}

/// Speed factor `elapsed` after the keys went down.
pub fn ramp(elapsed: Duration) -> f64 {
    let t = (elapsed.as_secs_f64() / RAMP.as_secs_f64()).min(1.0);
    START_SHARE + (1.0 - START_SHARE) * t
}

/// The view's direction for the held keys: their sum, one unit long.
pub fn heading(dirs: &[Dir]) -> Option<(f64, f64)> {
    let (x, y) = dirs.iter().fold((0.0, 0.0), |(x, y), d| {
        let (dx, dy) = d.vector();
        (x + dx, y + dy)
    });
    let len = (x * x + y * y).sqrt();
    (len > 1e-9).then(|| (x / len, y / len))
}

impl State {
    /// A pan key went down: the view glides until it is let go.
    pub fn key_pan_press(&mut self, key: Keycode, dir: Dir) {
        if self.keypan.held.iter().any(|(k, _)| *k == key) {
            return;
        }
        if self.keypan.held.is_empty() {
            self.keypan.since = Some(Instant::now());
            self.keypan.last_tick = None;
        }
        self.keypan.held.push((key, dir));
        if self.keypan.ticking {
            return;
        }
        self.keypan.ticking = true;
        let timer = self
            .loop_handle
            .insert_source(Timer::immediate(), |_, _, state| {
                if state.key_pan_tick() {
                    TimeoutAction::ToDuration(TICK)
                } else {
                    state.keypan.ticking = false;
                    TimeoutAction::Drop
                }
            });
        if timer.is_err() {
            self.keypan.ticking = false;
        }
    }

    /// A key went up; true when it was panning.
    pub fn key_pan_release(&mut self, key: Keycode) -> bool {
        let before = self.keypan.held.len();
        self.keypan.held.retain(|(k, _)| *k != key);
        before != self.keypan.held.len()
    }

    /// Moves the view one step; false once no pan key is held.
    fn key_pan_tick(&mut self) -> bool {
        // A release can get lost (a VT switch, the lock screen): only keys
        // still down count.
        if let Some(keyboard) = self.seat.get_keyboard() {
            let down = keyboard.pressed_keys();
            self.keypan.held.retain(|(k, _)| down.contains(k));
        }
        let dirs: Vec<Dir> = self.keypan.held.iter().map(|(_, d)| *d).collect();
        let Some((hx, hy)) = heading(&dirs) else {
            self.keypan.since = None;
            return false;
        };
        let Some(output) = self.active_output() else {
            return false;
        };
        let now = Instant::now();
        let dt = self
            .keypan
            .last_tick
            .map_or(TICK, |t| now.duration_since(t))
            .min(Duration::from_millis(50));
        self.keypan.last_tick = Some(now);
        let shift = self
            .seat
            .get_keyboard()
            .is_some_and(|k| k.modifier_state().shift);
        let speed = self.config.canvas.pan_speed
            * ramp(now.duration_since(self.keypan.since.unwrap_or(now)))
            * if shift { FAST } else { 1.0 };
        let step = speed * dt.as_secs_f64();
        // Keys move the view, so the canvas slides the other way.
        self.pan(&output, Point::from((-hx * step, -hy * step)), false);
        true
    }

    /// `navigate`: the mode where plain arrows and h j k l move the view.
    pub fn toggle_navigate(&mut self) {
        self.keypan.mode = !self.keypan.mode;
        if self.keypan.mode {
            self.spawn(
                "notify-send -a pzwm -t 4000 -h string:x-canonical-private-synchronous:pzwm-nav \
                 'Navigate' 'Arrows or h j k l: move · Shift: faster · + −: zoom · 0: home · Esc: done'",
            );
        } else {
            self.keypan.held.clear();
            self.spawn(
                "notify-send -a pzwm -t 1200 -h string:x-canonical-private-synchronous:pzwm-nav 'Navigate: done'",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_keys_and_arrows_pan() {
        assert_eq!(nav_key(Keysym::h), Some(NavKey::Pan(Dir::Left)));
        assert_eq!(nav_key(Keysym::Down), Some(NavKey::Pan(Dir::Down)));
        assert_eq!(nav_key(Keysym::K), Some(NavKey::Pan(Dir::Up)));
        assert_eq!(nav_key(Keysym::l), Some(NavKey::Pan(Dir::Right)));
        assert_eq!(nav_key(Keysym::Escape), Some(NavKey::Exit));
        assert_eq!(nav_key(Keysym::a), None);
    }

    #[test]
    fn held_keys_combine_into_one_heading() {
        let (x, y) = heading(&[Dir::Right, Dir::Down]).unwrap();
        assert!((x - y).abs() < 1e-9 && (x * x + y * y - 1.0).abs() < 1e-9);
        // Opposite keys cancel out.
        assert_eq!(heading(&[Dir::Left, Dir::Right]), None);
    }

    #[test]
    fn a_held_pan_speeds_up_to_full_speed() {
        assert!((ramp(Duration::ZERO) - START_SHARE).abs() < 1e-9);
        assert!(ramp(RAMP / 2) > START_SHARE && ramp(RAMP / 2) < 1.0);
        assert_eq!(ramp(RAMP * 3), 1.0);
    }
}
