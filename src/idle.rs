// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! What happens when nobody touches the computer, `[idle]`: the screen
//! dims, turns off, the session locks and, on battery, the laptop sleeps.
//! Any input undoes the dimming and wakes the screens. While an app
//! inhibits idling (a playing video) nothing happens.
//!
//! Before any sleep (idle, lid, power menu, `systemctl suspend`) the
//! session locks: pzwm holds a logind "delay" inhibitor and watches logind
//! with `gdbus monitor`, so it can start the lock screen in time.
//! `loginctl lock-session` locks the same way.

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use smithay::reexports::calloop::{
    channel,
    timer::{TimeoutAction, Timer},
};
use tracing::{info, warn};

use crate::state::State;

/// How often the idle time is checked.
const TICK: Duration = Duration::from_secs(1);
/// Longest wait for the lock screen before sleep goes ahead anyway.
const LOCK_GRACE: Duration = Duration::from_secs(3);
/// Input this soon after the screens went off (the release of the key
/// that turned them off) does not wake them.
const WAKE_GRACE: Duration = Duration::from_millis(1000);
/// Wait between locking and an idle suspend without logind's signal.
const SUSPEND_AFTER_LOCK: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct IdleState {
    pub last_activity: Option<Instant>,
    pub dimmed: bool,
    pub screens_off: bool,
    screens_off_at: Option<Instant>,
    /// Steps already taken in this idle stretch.
    locked_by_idle: bool,
    suspended: bool,
    /// When the session locked, for `locked_screen_off_after`.
    pub locked_at: Option<Instant>,
    /// logind delay inhibitor (`systemd-inhibit … --mode=delay`).
    sleep_inhibitor: Option<Child>,
    /// logind is told to sleep and gdbus watches it.
    watching_logind: bool,
}

impl Drop for IdleState {
    fn drop(&mut self) {
        if let Some(mut child) = self.sleep_inhibitor.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// What logind said, from `gdbus monitor`.
#[derive(Debug, PartialEq, Eq)]
pub enum Logind {
    /// About to sleep (true) or woke up (false).
    PrepareForSleep(bool),
    /// `loginctl lock-session` for this session.
    Lock,
}

/// A `gdbus monitor` line, for the session at `session_path`.
pub fn parse_logind(line: &str, session_path: Option<&str>) -> Option<Logind> {
    if line.contains("org.freedesktop.login1.Manager.PrepareForSleep") {
        return Some(Logind::PrepareForSleep(line.contains("(true,)")));
    }
    if line.contains("org.freedesktop.login1.Session.Lock ") {
        let path = line.split(':').next()?.trim();
        return (Some(path) == session_path).then_some(Logind::Lock);
    }
    None
}

/// logind's object path of session `id` (`2` → `/org/freedesktop/login1/session/_32`).
pub fn session_object_path(id: &str) -> String {
    let mut out = String::from("/org/freedesktop/login1/session/");
    for (i, b) in id.bytes().enumerate() {
        if b.is_ascii_alphabetic() || (b.is_ascii_digit() && i > 0) {
            out.push(b as char);
        } else {
            out.push_str(&format!("_{b:02x}"));
        }
    }
    out
}

/// Whether the computer runs on mains power; a desktop without a battery
/// counts as plugged in.
pub fn on_ac_power(supplies: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(supplies) else {
        return true;
    };
    let mut has_mains = false;
    for e in entries.flatten() {
        let read = |f: &str| std::fs::read_to_string(e.path().join(f)).unwrap_or_default();
        let kind = read("type");
        if matches!(kind.trim(), "Mains" | "USB") {
            has_mains = true;
            if read("online").trim() == "1" {
                return true;
            }
        }
    }
    !has_mains
}

/// The lock command: `setting`, or for "auto" the first lock screen found.
pub fn lock_command(setting: &str) -> String {
    match setting.trim() {
        "" | "auto" => {
            "if command -v dms >/dev/null && dms ipc call lock lock >/dev/null 2>&1; then :; \
             elif command -v hyprlock >/dev/null; then pidof hyprlock >/dev/null || hyprlock; \
             elif command -v swaylock >/dev/null; then pidof swaylock >/dev/null || swaylock -f; fi"
                .into()
        }
        cmd => cmd.into(),
    }
}

impl State {
    /// Starts the idle timer and the logind watch.
    pub fn start_idle(&mut self) {
        self.idle.last_activity = Some(Instant::now());
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(TICK), |_, _, state| {
                state.idle_tick();
                TimeoutAction::ToDuration(TICK)
            });
        if let Err(err) = timer {
            warn!("idle timer: {err}");
        }
        self.watch_logind();
        self.take_sleep_inhibitor();
    }

    /// Input happened: undo dimming, wake the screens.
    pub fn idle_activity(&mut self) {
        self.idle.last_activity = Some(Instant::now());
        self.idle.locked_by_idle = false;
        self.idle.suspended = false;
        if self.idle.dimmed {
            self.idle.dimmed = false;
            let cmd = self.config.idle.undim_command.clone();
            if !cmd.trim().is_empty() {
                self.spawn(&cmd);
            }
        }
        let just_off = self
            .idle
            .screens_off_at
            .is_some_and(|t| t.elapsed() < WAKE_GRACE);
        if self.idle.screens_off && !just_off {
            self.set_screens_on(true);
        }
    }

    fn idle_tick(&mut self) {
        let cfg = self.config.idle.clone();
        if !cfg.enabled || !self.idle_inhibitors.is_empty() {
            return;
        }
        let now = Instant::now();
        let idle = now.duration_since(self.idle.last_activity.unwrap_or(now));
        let past = |secs: u64| secs > 0 && idle >= Duration::from_secs(secs);

        if past(cfg.dim_after) && !self.idle.dimmed && !self.idle.screens_off {
            self.idle.dimmed = true;
            if !cfg.dim_command.trim().is_empty() {
                self.spawn(&cfg.dim_command);
            }
        }
        let locked = self.lock.is_locked();
        let locked_for = self
            .idle
            .locked_at
            .map(|t| now.duration_since(t).min(idle))
            .unwrap_or_default();
        let off_when_locked = locked
            && cfg.locked_screen_off_after > 0
            && locked_for >= Duration::from_secs(cfg.locked_screen_off_after);
        if (past(cfg.screen_off_after) || off_when_locked) && !self.idle.screens_off {
            self.set_screens_on(false);
        }
        if past(cfg.lock_after) && !locked && !self.idle.locked_by_idle {
            self.idle.locked_by_idle = true;
            self.lock_session();
        }
        let battery_ok = cfg.suspend_on_ac || !on_ac_power(Path::new("/sys/class/power_supply"));
        if past(cfg.suspend_after) && !self.idle.suspended && battery_ok {
            self.idle.suspended = true;
            self.idle_suspend();
        }
    }

    /// Sleeps; locks first when logind cannot tell pzwm in time.
    fn idle_suspend(&mut self) {
        info!("idle: suspend");
        if self.config.idle.lock_before_sleep
            && !self.idle.watching_logind
            && !self.lock.is_locked()
        {
            self.lock_session();
            let _ = self.loop_handle.insert_source(
                Timer::from_duration(SUSPEND_AFTER_LOCK),
                |_, _, state| {
                    state.spawn("systemctl suspend");
                    TimeoutAction::Drop
                },
            );
            return;
        }
        self.spawn("systemctl suspend");
    }

    /// Starts the lock screen, unless the session is locked already.
    pub fn lock_session(&mut self) {
        if self.lock.is_locked() {
            return;
        }
        info!("locking the session");
        let cmd = lock_command(&self.config.idle.lock_command);
        self.spawn(&cmd);
    }

    /// The lock screen is up: sleep may go ahead.
    pub fn idle_locked(&mut self) {
        self.idle.locked_at = Some(Instant::now());
        self.release_sleep_inhibitor();
    }

    pub fn idle_unlocked(&mut self) {
        self.idle.locked_at = None;
        self.idle_activity();
    }

    /// `dpms on|off|toggle`.
    pub fn dpms_command(&mut self, arg: &str) -> Result<String, String> {
        let on = match arg.trim() {
            "on" => true,
            "off" => false,
            "" | "toggle" => self.idle.screens_off,
            _ => return Err("usage: dpms on|off|toggle".into()),
        };
        self.set_screens_on(on);
        Ok(if on { "screens on" } else { "screens off" }.into())
    }

    fn set_screens_on(&mut self, on: bool) {
        if self.idle.screens_off == !on {
            return;
        }
        info!("screens {}", if on { "on" } else { "off" });
        self.idle.screens_off = !on;
        self.idle.screens_off_at = (!on).then(Instant::now);
        crate::backend::set_screens_power(self, on);
        if on {
            self.queue_redraw_all();
        }
    }

    /// Holds sleep back a moment, so the lock screen is up before it.
    fn take_sleep_inhibitor(&mut self) {
        if !self.config.idle.lock_before_sleep || self.idle.sleep_inhibitor.is_some() {
            return;
        }
        // The shell ends with pzwm, so a crash leaves no inhibitor behind.
        let pid = std::process::id();
        let child = Command::new("systemd-inhibit")
            .args([
                "--what=sleep",
                "--mode=delay",
                "--who=pzwm",
                "--why=Lock the screen before sleep",
                "sh",
                "-c",
                &format!("while kill -0 {pid} 2>/dev/null; do sleep 5; done"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(child) => self.idle.sleep_inhibitor = Some(child),
            Err(err) => warn!("systemd-inhibit: {err}"),
        }
    }

    fn release_sleep_inhibitor(&mut self) {
        if let Some(mut child) = self.idle.sleep_inhibitor.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Follows logind's sleep and lock signals through `gdbus monitor`.
    fn watch_logind(&mut self) {
        let spawned = Command::new("gdbus")
            .args(["monitor", "--system", "--dest", "org.freedesktop.login1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(err) => {
                warn!("gdbus (glib2) missing, no lock before sleep from logind: {err}");
                return;
            }
        };
        let Some(stdout) = child.stdout.take() else {
            return;
        };
        let session = std::env::var("XDG_SESSION_ID")
            .ok()
            .map(|id| session_object_path(&id));
        let (tx, rx) = channel::channel::<Logind>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(event) = parse_logind(&line, session.as_deref()) {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            }
        });
        let added = self.loop_handle.insert_source(rx, |event, _, state| {
            if let channel::Event::Msg(event) = event {
                state.on_logind(event);
            }
        });
        if let Err(err) = added {
            warn!("logind watch: {err}");
            let _ = child.kill();
            return;
        }
        self.children.push(child);
        self.idle.watching_logind = true;
    }

    fn on_logind(&mut self, event: Logind) {
        match event {
            Logind::Lock => self.lock_session(),
            Logind::PrepareForSleep(true) => {
                info!("going to sleep");
                if self.config.idle.lock_before_sleep && !self.lock.is_locked() {
                    self.lock_session();
                    // Let sleep go ahead even if no lock screen comes up.
                    let _ = self.loop_handle.insert_source(
                        Timer::from_duration(LOCK_GRACE),
                        |_, _, state| {
                            state.release_sleep_inhibitor();
                            TimeoutAction::Drop
                        },
                    );
                } else {
                    self.release_sleep_inhibitor();
                }
            }
            Logind::PrepareForSleep(false) => {
                info!("woke up");
                self.idle_activity();
                self.take_sleep_inhibitor();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logind_lines_parse() {
        let sleep =
            "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (true,)";
        assert_eq!(
            parse_logind(sleep, None),
            Some(Logind::PrepareForSleep(true))
        );
        let wake =
            "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (false,)";
        assert_eq!(
            parse_logind(wake, None),
            Some(Logind::PrepareForSleep(false))
        );
        let lock = "/org/freedesktop/login1/session/_32: org.freedesktop.login1.Session.Lock ()";
        let ours = session_object_path("2");
        assert_eq!(parse_logind(lock, Some(&ours)), Some(Logind::Lock));
        // Another user's session is not ours to lock.
        assert_eq!(parse_logind(lock, Some(&session_object_path("7"))), None);
        assert_eq!(parse_logind("anything else", Some(&ours)), None);
    }

    #[test]
    fn session_paths_escape_leading_digits() {
        assert_eq!(
            session_object_path("2"),
            "/org/freedesktop/login1/session/_32"
        );
        assert_eq!(
            session_object_path("c1"),
            "/org/freedesktop/login1/session/c1"
        );
        assert_eq!(
            session_object_path("12"),
            "/org/freedesktop/login1/session/_312"
        );
    }

    #[test]
    fn ac_power_from_sysfs() {
        let dir = std::env::temp_dir().join(format!("pzwm-power-{}", std::process::id()));
        let mains = dir.join("AC");
        let bat = dir.join("BAT0");
        std::fs::create_dir_all(&mains).unwrap();
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(mains.join("type"), "Mains\n").unwrap();
        std::fs::write(bat.join("type"), "Battery\n").unwrap();
        std::fs::write(mains.join("online"), "0\n").unwrap();
        assert!(!on_ac_power(&dir));
        std::fs::write(mains.join("online"), "1\n").unwrap();
        assert!(on_ac_power(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
        // No power supplies at all: a desktop, plugged in.
        assert!(on_ac_power(Path::new("/nonexistent-pzwm")));
    }

    #[test]
    fn auto_lock_tries_the_bar_first() {
        assert!(lock_command("auto").starts_with("if command -v dms"));
        assert_eq!(lock_command("swaylock -f"), "swaylock -f");
    }
}
