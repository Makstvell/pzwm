// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Workspace feed for bars.
//!
//! Speaks the JSON-lines protocol DankMaterialShell uses for the Umbriel
//! compositor, so its workspace widget works under pzwm: every connected
//! client gets `{"event":"workspaces","data":[…]}` and
//! `{"event":"windows","data":[…]}` lines whenever either changes. The bar
//! finds the socket through `UMBRIEL_SOCKET` and switches workspaces with
//! `umbriel msg …`, which `pzwm` answers when called under that name.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    io::{ErrorKind, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};

use serde_json::{Value, json};
use smithay::reexports::calloop::{Interest, Mode, PostAction, generic::Generic};
use tracing::{info, warn};

use crate::{canvas::WORKSPACES, shell::Placement, state::State};

#[derive(Debug, Default)]
pub struct BarFeed {
    clients: Vec<UnixStream>,
    /// What the last lines were built from. Checked on every event-loop
    /// pass (every pointer motion), so it is kept cheap to compute.
    key: Vec<u64>,
    /// Last lines sent, so unchanged state is not resent.
    workspaces: String,
    windows: String,
}

pub fn socket_path(wayland_display: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join(format!("pzwm-bar-{wayland_display}.sock"))
}

pub fn init(state: &mut State) -> anyhow::Result<PathBuf> {
    let path = socket_path(&state.socket_name.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    state
        .loop_handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            |_, listener, state| {
                while let Ok((stream, _)) = listener.accept() {
                    state.bar_subscribe(stream);
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("bar source: {e}"))?;
    info!("bar socket {}", path.display());
    Ok(path)
}

/// Writes one line; `false` means the client is gone (or stuck) and is dropped.
fn send(stream: &mut UnixStream, line: &str) -> bool {
    match stream.write_all(line.as_bytes()) {
        Ok(()) => true,
        Err(err) if err.kind() == ErrorKind::WouldBlock => {
            warn!("bar client not reading, dropping it");
            false
        }
        Err(_) => false,
    }
}

impl State {
    fn bar_subscribe(&mut self, mut stream: UnixStream) {
        if stream.set_nonblocking(true).is_err() {
            return;
        }
        let (workspaces, windows) = self.bar_lines();
        if send(&mut stream, &workspaces) && send(&mut stream, &windows) {
            self.bar.clients.push(stream);
        }
        // Other clients already have this state or get it on the next change.
        self.bar.key = self.bar_key();
        self.bar.workspaces = workspaces;
        self.bar.windows = windows;
    }

    /// Sends the workspace and window lists to bars if they changed.
    pub fn bar_publish(&mut self) {
        if self.bar.clients.is_empty() {
            return;
        }
        let key = self.bar_key();
        if key == self.bar.key {
            return;
        }
        self.bar.key = key;
        let (workspaces, windows) = self.bar_lines();
        let mut changed = Vec::new();
        if workspaces != self.bar.workspaces {
            changed.push(workspaces.clone());
        }
        if windows != self.bar.windows {
            changed.push(windows.clone());
        }
        if changed.is_empty() {
            return;
        }
        self.bar
            .clients
            .retain_mut(|c| changed.iter().all(|line| send(c, line)));
        self.bar.workspaces = workspaces;
        self.bar.windows = windows;
    }

    /// Everything the lines depend on: per output its shown workspace and
    /// whether it is focused; per listed window its id, workspace, focus and
    /// app id.
    fn bar_key(&self) -> Vec<u64> {
        let focused_output = self.active_output();
        let mut key = Vec::with_capacity(self.outputs.len() * 2 + self.windows.len() * 4);
        for output in &self.outputs {
            key.push(self.output_ws(output) as u64);
            key.push(u64::from(focused_output.as_ref() == Some(output)));
        }
        key.push(u64::MAX);
        for m in &self.windows {
            if m.mapped && m.placement != Placement::Pinned {
                key.push(m.id);
                key.push(m.ws as u64);
                key.push(u64::from(self.focused.as_ref() == Some(&m.window)));
                let mut hasher = DefaultHasher::new();
                self.app_id(m).hash(&mut hasher);
                key.push(hasher.finish());
            }
        }
        key
    }

    fn bar_lines(&self) -> (String, String) {
        let line = |event: &str, data: Vec<Value>| {
            format!("{}\n", json!({ "event": event, "data": data }))
        };
        (
            line("workspaces", self.bar_workspaces()),
            line("windows", self.bar_windows()),
        )
    }

    /// Like Hyprland, a bar lists only the workspaces that exist there: the
    /// one its output shows and the ones holding windows.
    fn bar_workspaces(&self) -> Vec<Value> {
        let focused_output = self.active_output();
        let mut list = Vec::new();
        for output in &self.outputs {
            let shown = self.output_ws(output);
            for ws in 0..WORKSPACES {
                let active = shown == ws;
                let occupied = self
                    .windows
                    .iter()
                    .any(|m| m.mapped && m.ws == ws && m.placement != Placement::Pinned);
                if !active && !occupied {
                    continue;
                }
                list.push(json!({
                    "id": workspace_id(&output.name(), ws),
                    "index": ws + 1,
                    "name": "",
                    "named": false,
                    "output": output.name(),
                    "active": active,
                    "focused": active && focused_output.as_ref() == Some(output),
                    "occupied": occupied,
                }));
            }
        }
        list
    }

    fn bar_windows(&self) -> Vec<Value> {
        let Some(first) = self.outputs.first() else {
            return Vec::new();
        };
        self.windows
            .iter()
            .filter(|m| m.mapped && m.placement != Placement::Pinned)
            .map(|m| {
                // A window belongs to the output showing its workspace.
                let output = self
                    .outputs
                    .iter()
                    .find(|o| self.output_ws(o) == m.ws)
                    .unwrap_or(first);
                json!({
                    "id": m.id.to_string(),
                    "app_id": self.app_id(m).unwrap_or_default(),
                    "active": self.focused.as_ref() == Some(&m.window),
                    "workspace": workspace_id(&output.name(), m.ws),
                    // Only used for ordering: opening order, so dragging a
                    // window does not resend the list on every motion.
                    "x": m.id,
                    "y": 0,
                    "urgent": false,
                })
            })
            .collect()
    }
}

fn workspace_id(output: &str, ws: usize) -> String {
    format!("{output}:{}", ws + 1)
}

/// Turns an `umbriel msg <action>` from the bar into a pzwm request.
pub fn translate_umbriel(action: &str) -> Option<String> {
    let (name, arg) = action.split_once(':').unwrap_or((action, ""));
    match name {
        // "workspace-switch:3/eDP-1": the bar is clicked on that output,
        // which is the one under the cursor.
        "workspace-switch" => {
            let idx = arg.split('/').next()?.parse::<usize>().ok()?;
            Some(format!("workspace {idx}"))
        }
        "session-quit" => Some("quit".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::translate_umbriel;

    #[test]
    fn workspace_switch_maps_to_workspace_request() {
        assert_eq!(
            translate_umbriel("workspace-switch:3/eDP-1").as_deref(),
            Some("workspace 3")
        );
    }

    #[test]
    fn session_quit_maps_to_quit() {
        assert_eq!(
            translate_umbriel("session-quit:skip-confirmation").as_deref(),
            Some("quit")
        );
    }

    #[test]
    fn unknown_actions_are_rejected() {
        assert_eq!(translate_umbriel("dpms-off"), None);
        assert_eq!(translate_umbriel("workspace-switch:x/eDP-1"), None);
    }
}
