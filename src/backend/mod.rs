// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Display/input backends: `udev` (real session on a TTY) and `winit`
//! (a window inside another compositor, for testing).

pub mod udev;
pub mod winit;

use smithay::{
    backend::allocator::dmabuf::Dmabuf, output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
};

use crate::state::State;

pub enum Backend {
    Winit(Box<winit::WinitData>),
    Udev(Box<udev::UdevData>),
    /// Temporarily moved out while rendering.
    Taken,
}

impl Backend {
    pub fn seat_name(&self) -> String {
        match self {
            Backend::Udev(data) => data.seat_name(),
            _ => "seat0".into(),
        }
    }
}

pub fn queue_redraw(state: &mut State, output: &Output) {
    match &state.backend {
        Backend::Winit(_) => winit::queue_redraw(state),
        Backend::Udev(_) => udev::queue_redraw(state, output),
        // A render is in progress; it re-checks for pending work when done.
        Backend::Taken => crate::state::output_state(output).redraw_pending = true,
    }
}

pub fn early_import(state: &mut State, surface: &WlSurface) {
    if let Backend::Udev(data) = &mut state.backend {
        data.early_import(surface);
    }
}

pub fn import_dmabuf(state: &mut State, dmabuf: &Dmabuf) -> bool {
    match &mut state.backend {
        Backend::Winit(data) => data.import_dmabuf(dmabuf),
        Backend::Udev(data) => data.import_dmabuf(dmabuf),
        Backend::Taken => false,
    }
}

pub fn switch_vt(state: &mut State, vt: i32) {
    if let Backend::Udev(data) = &mut state.backend {
        data.switch_vt(vt);
    }
}

/// A monitor as output-management clients see it, connected or not
/// enabled.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadInfo {
    pub name: String,
    pub make: String,
    pub model: String,
    /// Millimetres.
    pub physical_size: (i32, i32),
    pub modes: Vec<ModeInfo>,
    /// Index into `modes` while enabled.
    pub current_mode: Option<usize>,
    pub enabled: bool,
    pub position: (i32, i32),
    pub scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeInfo {
    pub width: i32,
    pub height: i32,
    /// mHz.
    pub refresh: i32,
    pub preferred: bool,
}

/// Every monitor pzwm could drive.
pub fn heads(state: &State) -> Vec<HeadInfo> {
    match &state.backend {
        Backend::Udev(data) => udev::heads(state, data),
        _ => state
            .outputs
            .iter()
            .map(|o| {
                let mode = o.current_mode().unwrap_or(smithay::output::Mode {
                    size: (0, 0).into(),
                    refresh: 0,
                });
                let g = state.output_geometry(o);
                HeadInfo {
                    name: o.name(),
                    make: o.physical_properties().make,
                    model: o.physical_properties().model,
                    physical_size: (0, 0),
                    modes: vec![ModeInfo {
                        width: mode.size.w,
                        height: mode.size.h,
                        refresh: mode.refresh,
                        preferred: true,
                    }],
                    current_mode: Some(0),
                    enabled: true,
                    position: (g.loc.x, g.loc.y),
                    scale: o.current_scale().fractional_scale(),
                }
            })
            .collect(),
    }
}

/// Applies changed monitor settings (config reload or output management):
/// modes, enabling and mirroring on a TTY, scale and position everywhere.
pub fn reapply_outputs(state: &mut State) {
    if matches!(state.backend, Backend::Udev(_)) {
        udev::reapply_outputs(state);
    }
    state.apply_output_scales();
    state.relayout_outputs();
}

/// Length of the output's gamma ramp, `None` when it cannot be set.
pub fn gamma_size(state: &State, output: &Output) -> Option<u32> {
    match &state.backend {
        Backend::Udev(data) => udev::gamma_size(data, output),
        _ => None,
    }
}

/// Sets the gamma ramp (red, green, blue, `gamma_size` values each), or
/// restores a linear one with `None`.
pub fn set_gamma(state: &State, output: &Output, ramp: Option<&[u16]>) -> Result<(), String> {
    match &state.backend {
        Backend::Udev(data) => udev::set_gamma(data, output, ramp),
        _ => Err("gamma needs a TTY session".into()),
    }
}

/// Turns every screen off (DPMS) or back on.
pub fn set_screens_power(state: &mut State, on: bool) {
    if matches!(state.backend, Backend::Udev(_)) {
        udev::set_screens_power(state, on);
    }
}

pub fn reconfigure_input(state: &mut State) {
    let input = state.config.input.clone();
    if let Backend::Udev(data) = &mut state.backend {
        data.configure_devices(&input);
    }
}
