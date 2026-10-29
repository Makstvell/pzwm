//! Event stream for scripts: `pzwm msg subscribe` keeps the connection open
//! and gets one JSON object per line whenever something changes:
//!
//! - `{"event":"state","windows":[…],"outputs":[…]}` first, the full state;
//! - `window-opened` / `window-changed` with `"window":{…}`;
//! - `window-closed` with `"id"`;
//! - `focus-changed` with `"id"` (null when nothing is focused);
//! - `workspace-changed` with `"output"` and `"workspace"` (1 … 10).
//!
//! The state is compared after every event-loop pass, like the bar feed.

use std::{
    collections::BTreeMap,
    io::{ErrorKind, Write},
    os::unix::net::UnixStream,
};

use serde_json::{Value, json};
use tracing::warn;

use crate::{shell::Managed, state::State};

#[derive(Debug, Default)]
pub struct EventFeed {
    clients: Vec<UnixStream>,
    last: Snapshot,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Snapshot {
    windows: BTreeMap<u64, Value>,
    outputs: BTreeMap<String, usize>,
    focused: Option<u64>,
}

/// Lines describing how `old` became `new`.
fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Value> {
    let mut events = Vec::new();
    for (id, w) in &new.windows {
        match old.windows.get(id) {
            None => events.push(json!({ "event": "window-opened", "window": w })),
            Some(prev) if prev != w => {
                events.push(json!({ "event": "window-changed", "window": w }))
            }
            Some(_) => {}
        }
    }
    for id in old
        .windows
        .keys()
        .filter(|id| !new.windows.contains_key(id))
    {
        events.push(json!({ "event": "window-closed", "id": id }));
    }
    for (name, ws) in &new.outputs {
        if old.outputs.get(name) != Some(ws) {
            events.push(json!({
                "event": "workspace-changed",
                "output": name,
                "workspace": ws + 1,
            }));
        }
    }
    if old.focused != new.focused {
        events.push(json!({ "event": "focus-changed", "id": new.focused }));
    }
    events
}

/// Writes one line; `false` drops the client (gone or not reading).
fn send(stream: &mut UnixStream, line: &str) -> bool {
    match stream.write_all(line.as_bytes()) {
        Ok(()) => true,
        Err(err) if err.kind() == ErrorKind::WouldBlock => {
            warn!("event subscriber not reading, dropping it");
            false
        }
        Err(_) => false,
    }
}

impl State {
    pub fn window_json(&self, m: &Managed) -> Value {
        let size = m.size();
        json!({
            "id": m.id,
            "app_id": self.app_id(m).unwrap_or_default(),
            "title": self.title(m).unwrap_or_default(),
            "workspace": m.ws + 1,
            "placement": format!("{:?}", m.placement).to_lowercase(),
            "x": m.pos.x.round(),
            "y": m.pos.y.round(),
            "width": size.w,
            "height": size.h,
            "focused": self.focused.as_ref() == Some(&m.window),
            "group": m.group,
            "hidden": !self.shown(m),
            "scratchpad": m.scratchpad,
        })
    }

    pub fn outputs_json(&self) -> Vec<Value> {
        self.outputs
            .iter()
            .map(|o| {
                let g = self.output_geometry(o);
                let cam = self.camera(o);
                json!({
                    "name": o.name(),
                    "x": g.loc.x,
                    "y": g.loc.y,
                    "width": g.size.w,
                    "height": g.size.h,
                    "scale": o.current_scale().fractional_scale(),
                    "workspace": self.output_ws(o) + 1,
                    "camera": { "x": cam.pos.x, "y": cam.pos.y, "zoom": cam.zoom },
                })
            })
            .collect()
    }

    fn snapshot(&self) -> Snapshot {
        // Geometry is left out: a drag would send an event per motion.
        let windows = self
            .windows
            .iter()
            .filter(|m| m.mapped)
            .map(|m| {
                let mut w = self.window_json(m);
                for key in ["x", "y", "width", "height", "focused"] {
                    w.as_object_mut().map(|o| o.remove(key));
                }
                (m.id, w)
            })
            .collect();
        let outputs = self
            .outputs
            .iter()
            .map(|o| (o.name(), self.output_ws(o)))
            .collect();
        let focused = self.focused_index().map(|i| self.windows[i].id);
        Snapshot {
            windows,
            outputs,
            focused,
        }
    }

    /// Adds a `subscribe` client and sends it the full state.
    pub fn events_subscribe(&mut self, mut stream: UnixStream) {
        if stream.set_nonblocking(true).is_err() {
            return;
        }
        let windows: Vec<Value> = self
            .windows
            .iter()
            .filter(|m| m.mapped)
            .map(|m| self.window_json(m))
            .collect();
        let line = format!(
            "{}\n",
            json!({ "event": "state", "windows": windows, "outputs": self.outputs_json() })
        );
        if send(&mut stream, &line) {
            if self.events.clients.is_empty() {
                self.events.last = self.snapshot();
            }
            self.events.clients.push(stream);
        }
    }

    /// Sends what changed since the last pass to subscribers.
    pub fn events_publish(&mut self) {
        if self.events.clients.is_empty() {
            return;
        }
        let now = self.snapshot();
        if now == self.events.last {
            return;
        }
        let lines: Vec<String> = diff(&self.events.last, &now)
            .into_iter()
            .map(|e| format!("{e}\n"))
            .collect();
        self.events.last = now;
        self.events
            .clients
            .retain_mut(|c| lines.iter().all(|line| send(c, line)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(windows: &[(u64, &str)], ws: usize, focused: Option<u64>) -> Snapshot {
        Snapshot {
            windows: windows
                .iter()
                .map(|(id, title)| (*id, json!({ "id": id, "title": title })))
                .collect(),
            outputs: [("eDP-1".to_string(), ws)].into_iter().collect(),
            focused,
        }
    }

    fn names(events: &[Value]) -> Vec<&str> {
        events
            .iter()
            .map(|e| e["event"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn nothing_changed_means_no_events() {
        let s = snap(&[(1, "a")], 0, Some(1));
        assert!(diff(&s, &s).is_empty());
    }

    #[test]
    fn opening_closing_and_changing_windows_are_reported() {
        let old = snap(&[(1, "a"), (2, "b")], 0, Some(1));
        let new = snap(&[(1, "a2"), (3, "c")], 0, Some(1));
        let events = diff(&old, &new);
        assert_eq!(
            names(&events),
            ["window-changed", "window-opened", "window-closed"]
        );
        assert_eq!(events[2]["id"], 2);
    }

    #[test]
    fn workspace_and_focus_changes_are_reported() {
        let old = snap(&[(1, "a")], 0, Some(1));
        let new = snap(&[(1, "a")], 2, None);
        let events = diff(&old, &new);
        assert_eq!(names(&events), ["workspace-changed", "focus-changed"]);
        assert_eq!(events[0]["workspace"], 3);
        assert!(events[1]["id"].is_null());
    }
}
