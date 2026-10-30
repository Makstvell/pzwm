// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Scratchpad: one drop-down window (a terminal by default) that the
//! `scratchpad` action calls up at the top of the screen and puts away
//! again. It is started on first use by `[scratchpad] command`.

use std::time::{Duration, Instant};

use smithay::utils::{Point, Size};
use tracing::info;

use crate::{
    shell::{Placement, share_of},
    state::State,
};

/// A second press this soon after starting the app does not start another.
const SPAWN_GRACE: Duration = Duration::from_secs(3);

impl State {
    pub fn toggle_scratchpad(&mut self) {
        let Some(idx) = self.windows.iter().position(|m| m.scratchpad && m.mapped) else {
            self.spawn_scratchpad();
            return;
        };
        let active = self.active_output().map(|o| o.name());
        let m = &self.windows[idx];
        let visible_here = !m.hidden && m.output.is_some() && m.output == active;
        if visible_here {
            self.hide_scratchpad(idx);
        } else {
            self.show_scratchpad(idx);
        }
    }

    fn spawn_scratchpad(&mut self) {
        if self
            .scratchpad_spawned
            .is_some_and(|t| t.elapsed() < SPAWN_GRACE)
        {
            return;
        }
        let command = self.config.scratchpad.command.trim().to_string();
        if command.is_empty() {
            info!("scratchpad: no window and no [scratchpad] command");
            return;
        }
        self.scratchpad_spawned = Some(Instant::now());
        self.spawn(&command);
    }

    /// Turns a freshly mapped window into the scratchpad, shown.
    pub fn adopt_scratchpad(&mut self, idx: usize) {
        // Only one scratchpad: an older one goes back to being a window.
        for m in self.windows.iter_mut().filter(|m| m.scratchpad) {
            m.scratchpad = false;
            m.hidden = false;
        }
        self.windows[idx].scratchpad = true;
        self.windows[idx].group = None;
        self.scratchpad_spawned = None;
        self.show_scratchpad(idx);
    }

    fn show_scratchpad(&mut self, idx: usize) {
        let Some(output) = self.active_output() else {
            return;
        };
        let area = self.usable_area(&output).to_f64();
        let gap = self.config.appearance.gaps as f64;
        let cfg = &self.config.scratchpad;
        let title = self.title_height(&self.windows[idx]);
        let size = Size::from((
            share_of(area.size.w, cfg.width, 1, 0),
            share_of(area.size.h - title, cfg.height, 1, 0),
        ));
        let x = area.loc.x + (area.size.w - size.w as f64) / 2.0;
        let m = &mut self.windows[idx];
        m.placement = Placement::Pinned;
        m.output = Some(output.name());
        m.pos = Point::from((x, area.loc.y + gap + title));
        m.hidden = false;
        m.restore = None;
        self.configure(idx, Some(size), false, false);
        let window = self.windows[idx].window.clone();
        self.focus_window(Some(window));
    }

    fn hide_scratchpad(&mut self, idx: usize) {
        self.windows[idx].hidden = true;
        let window = self.windows[idx].window.clone();
        if self.focused.as_ref() == Some(&window) {
            let ws = self.active_output().map(|o| self.output_ws(&o));
            let next = self
                .windows
                .iter()
                .find(|m| self.shown(m) && (Some(m.ws) == ws || m.placement == Placement::Pinned))
                .map(|m| m.window.clone());
            self.focus_window(next);
        }
        self.queue_redraw_all();
    }
}
