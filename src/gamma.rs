// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! wlr-gamma-control: lets night-light tools (wlsunset, gammastep) tint
//! the screen by setting each monitor's gamma ramp. Works on a TTY
//! session; the ramp goes back to linear when the tool exits.

use std::{collections::HashMap, fs::File, io::Read, os::unix::fs::FileExt};

use smithay::{
    output::Output,
    reexports::{
        wayland_protocols_wlr::gamma_control::v1::server::{
            zwlr_gamma_control_manager_v1::{self, ZwlrGammaControlManagerV1},
            zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, backend::ClientId,
        },
    },
};
use tracing::warn;

use crate::state::State;

/// One active control per monitor, by output name.
#[derive(Debug, Default)]
pub struct GammaControls {
    active: HashMap<String, ZwlrGammaControlV1>,
}

pub struct GammaData {
    output: String,
    /// Entries per channel.
    size: u32,
}

pub fn init(dh: &DisplayHandle) {
    dh.create_global::<State, ZwlrGammaControlManagerV1, _>(1, ());
}

/// Native-endian u16 values from raw bytes.
fn to_ramp(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect()
}

/// Reads a ramp from the client's file: from the start if it can seek
/// (a memfd), else as a stream.
fn read_ramp(file: &mut File, size: u32) -> std::io::Result<Vec<u16>> {
    let mut bytes = vec![0u8; size as usize * 3 * 2];
    if file.read_exact_at(&mut bytes, 0).is_err() {
        file.read_exact(&mut bytes)?;
    }
    Ok(to_ramp(&bytes))
}

impl GlobalDispatch<ZwlrGammaControlManagerV1, ()> for State {
    fn bind(
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrGammaControlManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for State {
    fn request(
        state: &mut State,
        _client: &Client,
        _manager: &ZwlrGammaControlManagerV1,
        request: zwlr_gamma_control_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let zwlr_gamma_control_manager_v1::Request::GetGammaControl { id, output } = request else {
            return;
        };
        let output = Output::from_resource(&output);
        let name = output.as_ref().map(|o| o.name()).unwrap_or_default();
        let size = output
            .as_ref()
            .and_then(|o| crate::backend::gamma_size(state, o))
            .unwrap_or(0);
        let taken = state.gamma.active.contains_key(&name);
        let control = data_init.init(
            id,
            GammaData {
                output: name.clone(),
                size,
            },
        );
        if size == 0 || taken {
            control.failed();
            return;
        }
        control.gamma_size(size);
        state.gamma.active.insert(name, control);
    }
}

impl Dispatch<ZwlrGammaControlV1, GammaData> for State {
    fn request(
        state: &mut State,
        _client: &Client,
        control: &ZwlrGammaControlV1,
        request: zwlr_gamma_control_v1::Request,
        data: &GammaData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        let zwlr_gamma_control_v1::Request::SetGamma { fd } = request else {
            return;
        };
        if state.gamma.active.get(&data.output) != Some(control) {
            return;
        }
        let result = read_ramp(&mut File::from(fd), data.size)
            .map_err(|e| e.to_string())
            .and_then(|ramp| {
                let output = state.output_by_name(&data.output).ok_or("output is gone")?;
                crate::backend::set_gamma(state, &output, Some(&ramp))
            });
        if let Err(err) = result {
            warn!("gamma on {}: {err}", data.output);
            state.gamma.active.remove(&data.output);
            control.failed();
        }
    }

    fn destroyed(
        state: &mut State,
        _client: ClientId,
        control: &ZwlrGammaControlV1,
        data: &GammaData,
    ) {
        if state.gamma.active.get(&data.output) == Some(control) {
            state.gamma.active.remove(&data.output);
            if let Some(output) = state.output_by_name(&data.output) {
                let _ = crate::backend::set_gamma(state, &output, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_bytes_are_native_u16s() {
        let values: [u16; 3] = [0, 0x1234, u16::MAX];
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        assert_eq!(to_ramp(&bytes), values);
    }

    #[test]
    fn ramp_is_read_from_the_start_of_a_file() {
        let dir = std::env::temp_dir().join(format!("pzwm-gamma-test-{}", std::process::id()));
        let values: Vec<u16> = (0..6).map(|i| i * 1000).collect();
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        std::fs::write(&dir, &bytes).unwrap();
        let mut file = File::open(&dir).unwrap();
        // Already at the end, as a client that just wrote it would leave it.
        file.read_to_end(&mut Vec::new()).unwrap();
        assert_eq!(read_ramp(&mut file, 2).unwrap(), values);
        std::fs::remove_file(&dir).unwrap();
    }
}
