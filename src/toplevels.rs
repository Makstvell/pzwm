// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Window lists for taskbars and docks.
//!
//! Two protocols describe the open windows to other programs:
//! `ext-foreign-toplevel-list` (read-only, from smithay) and
//! `wlr-foreign-toplevel-management`, which also lets a taskbar activate,
//! close, maximize, minimize and fullscreen windows (waybar's
//! `wlr/taskbar`, Quickshell docks such as DankMaterialShell's). Both are
//! refreshed from the window list after every event-loop pass.
//!
//! Minimize puts a window away (`hidden`) like the scratchpad; activating
//! it from the taskbar or `find` brings it back.

use std::collections::HashMap;

use smithay::{
    output::Output,
    reexports::{
        wayland_protocols_wlr::foreign_toplevel::v1::server::{
            zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
            zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
        },
    },
    wayland::foreign_toplevel_list::{
        ForeignToplevelHandle, ForeignToplevelListHandler, ForeignToplevelListState,
    },
};

use crate::{shell::Placement, state::State};

const WLR_VERSION: u32 = 3;

/// What a taskbar is told about one window.
#[derive(Debug, Clone, Default, PartialEq)]
struct Info {
    title: String,
    app_id: String,
    maximized: bool,
    minimized: bool,
    activated: bool,
    fullscreen: bool,
    outputs: Vec<String>,
}

impl Info {
    fn states(&self) -> Vec<u8> {
        use zwlr_foreign_toplevel_handle_v1::State as S;
        [
            (self.maximized, S::Maximized),
            (self.minimized, S::Minimized),
            (self.activated, S::Activated),
            (self.fullscreen, S::Fullscreen),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .flat_map(|(_, s)| (s as u32).to_ne_bytes())
        .collect()
    }
}

struct Entry {
    info: Info,
    ext: ForeignToplevelHandle,
    wlr: Vec<ZwlrForeignToplevelHandleV1>,
}

pub struct Toplevels {
    ext_state: ForeignToplevelListState,
    managers: Vec<ZwlrForeignToplevelManagerV1>,
    entries: HashMap<u64, Entry>,
}

impl Toplevels {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<State, ZwlrForeignToplevelManagerV1, _>(WLR_VERSION, ());
        Self {
            ext_state: ForeignToplevelListState::new::<State>(dh),
            managers: Vec::new(),
            entries: HashMap::new(),
        }
    }
}

fn outputs_named(state: &State, names: &[String]) -> Vec<Output> {
    names
        .iter()
        .filter_map(|n| state.output_by_name(n))
        .collect()
}

/// Sends everything about a window to a new wlr handle.
fn send_all(state: &State, handle: &ZwlrForeignToplevelHandleV1, info: &Info) {
    handle.title(info.title.clone());
    handle.app_id(info.app_id.clone());
    if let Some(client) = handle.client() {
        for output in outputs_named(state, &info.outputs) {
            for wl in output.client_outputs(&client) {
                handle.output_enter(&wl);
            }
        }
    }
    handle.state(info.states());
    handle.done();
}

/// Sends what changed between `old` and `new` to a wlr handle.
fn send_changes(state: &State, handle: &ZwlrForeignToplevelHandleV1, old: &Info, new: &Info) {
    if old.title != new.title {
        handle.title(new.title.clone());
    }
    if old.app_id != new.app_id {
        handle.app_id(new.app_id.clone());
    }
    if old.outputs != new.outputs {
        if let Some(client) = handle.client() {
            for (names, enter) in [(&old.outputs, false), (&new.outputs, true)] {
                let other = if enter { &old.outputs } else { &new.outputs };
                let changed: Vec<String> = names
                    .iter()
                    .filter(|n| !other.contains(n))
                    .cloned()
                    .collect();
                for output in outputs_named(state, &changed) {
                    for wl in output.client_outputs(&client) {
                        if enter {
                            handle.output_enter(&wl);
                        } else {
                            handle.output_leave(&wl);
                        }
                    }
                }
            }
        }
    }
    handle.state(new.states());
    handle.done();
}

impl State {
    fn toplevel_infos(&self) -> Vec<(u64, Info)> {
        self.windows
            .iter()
            .filter(|m| m.mapped && !m.scratchpad)
            .map(|m| {
                let outputs = self
                    .outputs
                    .iter()
                    .filter(|o| match m.placement {
                        Placement::Pinned => m.output.as_deref() == Some(o.name().as_str()),
                        _ => self.output_ws(o) == m.ws,
                    })
                    .map(|o| o.name())
                    .collect();
                let info = Info {
                    title: self.title(m).unwrap_or_default(),
                    app_id: self.app_id(m).unwrap_or_default(),
                    maximized: m.placement == Placement::Maximized,
                    minimized: m.hidden,
                    activated: self.focused.as_ref() == Some(&m.window),
                    fullscreen: m.placement == Placement::Fullscreen,
                    outputs,
                };
                (m.id, info)
            })
            .collect()
    }

    /// Brings both window lists up to date.
    pub fn toplevels_refresh(&mut self) {
        let infos = self.toplevel_infos();
        let gone: Vec<u64> = self
            .toplevels
            .entries
            .keys()
            .filter(|id| !infos.iter().any(|(i, _)| i == *id))
            .copied()
            .collect();
        for id in gone {
            if let Some(entry) = self.toplevels.entries.remove(&id) {
                for h in &entry.wlr {
                    h.closed();
                }
                self.toplevels.ext_state.remove_toplevel(&entry.ext);
            }
        }
        for (id, info) in infos {
            match self.toplevels.entries.get(&id) {
                Some(entry) if entry.info == info => {}
                Some(entry) => {
                    if entry.info.title != info.title {
                        entry.ext.send_title(&info.title);
                    }
                    if entry.info.app_id != info.app_id {
                        entry.ext.send_app_id(&info.app_id);
                    }
                    entry.ext.send_done();
                    for h in &entry.wlr {
                        send_changes(self, h, &entry.info, &info);
                    }
                    if let Some(entry) = self.toplevels.entries.get_mut(&id) {
                        entry.info = info;
                    }
                }
                None => {
                    let ext = self
                        .toplevels
                        .ext_state
                        .new_toplevel::<State>(&info.title, &info.app_id);
                    let managers = self.toplevels.managers.clone();
                    let wlr = managers
                        .iter()
                        .filter_map(|m| self.wlr_handle_for(m, id, &info))
                        .collect();
                    self.toplevels.entries.insert(id, Entry { info, ext, wlr });
                }
            }
        }
    }

    fn wlr_handle_for(
        &self,
        manager: &ZwlrForeignToplevelManagerV1,
        id: u64,
        info: &Info,
    ) -> Option<ZwlrForeignToplevelHandleV1> {
        let client = manager.client()?;
        let handle = client
            .create_resource::<ZwlrForeignToplevelHandleV1, u64, State>(
                &self.display_handle,
                manager.version(),
                id,
            )
            .ok()?;
        manager.toplevel(&handle);
        send_all(self, &handle, info);
        Some(handle)
    }

    fn toplevel_request(&mut self, id: u64, request: zwlr_foreign_toplevel_handle_v1::Request) {
        use zwlr_foreign_toplevel_handle_v1::Request;
        let Some(idx) = self.index_of_id(id) else {
            return;
        };
        match request {
            Request::Activate { .. } | Request::UnsetMinimized => {
                self.windows[idx].hidden = false;
                self.go_to_window(idx);
            }
            Request::SetMinimized => {
                if self.windows[idx].scratchpad {
                    return;
                }
                self.windows[idx].hidden = true;
                let window = self.windows[idx].window.clone();
                if self.focused.as_ref() == Some(&window) {
                    let ws = self.windows[idx].ws;
                    let next = self
                        .windows
                        .iter()
                        .find(|m| self.shown(m) && m.ws == ws)
                        .map(|m| m.window.clone());
                    self.focus_window(next);
                }
                self.queue_redraw_all();
            }
            Request::Close => {
                if let Some(t) = self.windows[idx].window.toplevel() {
                    t.send_close();
                }
            }
            Request::SetMaximized => {
                if self.windows[idx].placement != Placement::Maximized {
                    self.maximize(idx);
                }
            }
            Request::UnsetMaximized => {
                if self.windows[idx].placement == Placement::Maximized {
                    self.restore(idx);
                }
            }
            Request::SetFullscreen { .. } => {
                if self.windows[idx].placement != Placement::Fullscreen {
                    self.fullscreen(idx);
                }
            }
            Request::UnsetFullscreen => {
                if self.windows[idx].placement == Placement::Fullscreen {
                    self.restore(idx);
                }
            }
            Request::SetRectangle { .. } | Request::Destroy => {}
            _ => {}
        }
    }
}

impl ForeignToplevelListHandler for State {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.toplevels.ext_state
    }
}
smithay::delegate_foreign_toplevel_list!(State);

impl GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn bind(
        state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        let manager = data_init.init(resource, ());
        let infos: Vec<(u64, Info)> = state
            .toplevels
            .entries
            .iter()
            .map(|(id, e)| (*id, e.info.clone()))
            .collect();
        for (id, info) in infos {
            if let Some(handle) = state.wlr_handle_for(&manager, id, &info) {
                if let Some(entry) = state.toplevels.entries.get_mut(&id) {
                    entry.wlr.push(handle);
                }
            }
        }
        state.toplevels.managers.push(manager);
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn request(
        state: &mut State,
        _client: &Client,
        manager: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            state.toplevels.managers.retain(|m| m != manager);
            manager.finished();
        }
    }

    fn destroyed(
        state: &mut State,
        _client: smithay::reexports::wayland_server::backend::ClientId,
        manager: &ZwlrForeignToplevelManagerV1,
        _data: &(),
    ) {
        state.toplevels.managers.retain(|m| m != manager);
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, u64> for State {
    fn request(
        state: &mut State,
        _client: &Client,
        _handle: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        id: &u64,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        state.toplevel_request(*id, request);
    }

    fn destroyed(
        state: &mut State,
        _client: smithay::reexports::wayland_server::backend::ClientId,
        handle: &ZwlrForeignToplevelHandleV1,
        id: &u64,
    ) {
        if let Some(entry) = state.toplevels.entries.get_mut(id) {
            entry.wlr.retain(|h| h != handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_sent_as_native_u32s() {
        let info = Info {
            activated: true,
            maximized: true,
            ..Default::default()
        };
        let bytes = info.states();
        let values: Vec<u32> = bytes
            .chunks(4)
            .map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(values, [0, 2]);
        assert!(Info::default().states().is_empty());
    }
}
