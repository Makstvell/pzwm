//! Monitor changes while running: listing every connected monitor for
//! output management, re-applying `[[output]]` settings without replugging,
//! and gamma ramps (night light).

use smithay::{
    backend::drm::DrmNode,
    output::{Mode as WlMode, Output},
    reexports::drm::control::{Device as ControlDevice, ModeTypeFlags, connector, crtc},
};
use tracing::{info, warn};

use super::{Backend, OutputId, UdevData, connector_connected, connector_disconnected, pick_mode};
use crate::{
    backend::{HeadInfo, ModeInfo},
    state::{State, output_state},
};

fn connector_name(info: &connector::Info) -> String {
    format!("{}-{}", info.interface().as_str(), info.interface_id())
}

pub fn heads(state: &State, data: &UdevData) -> Vec<HeadInfo> {
    let mut heads = Vec::new();
    for device in data.devices.values() {
        for (info, crtc) in device.scanner.crtcs() {
            let name = connector_name(info);
            let modes: Vec<ModeInfo> = info
                .modes()
                .iter()
                .map(|m| {
                    let wl = WlMode::from(*m);
                    ModeInfo {
                        width: wl.size.w,
                        height: wl.size.h,
                        refresh: wl.refresh,
                        preferred: m.mode_type().contains(ModeTypeFlags::PREFERRED),
                    }
                })
                .collect();
            let output = device.surfaces.get(&crtc).map(|s| &s.output);
            let current_mode = output.and_then(|o| o.current_mode()).and_then(|cur| {
                modes.iter().position(|m| {
                    m.width == cur.size.w && m.height == cur.size.h && m.refresh == cur.refresh
                })
            });
            let (position, scale) = match output {
                Some(o) => {
                    let loc = o.current_location();
                    ((loc.x, loc.y), o.current_scale().fractional_scale())
                }
                None => (
                    state.output_config(&name).position().unwrap_or((0, 0)),
                    state.output_scale(&name),
                ),
            };
            let (w, h) = info.size().unwrap_or((0, 0));
            heads.push(HeadInfo {
                make: output
                    .map(|o| o.physical_properties().make)
                    .unwrap_or_default(),
                model: name.clone(),
                name,
                physical_size: (w as i32, h as i32),
                modes,
                current_mode,
                enabled: output.is_some(),
                position,
                scale,
            });
        }
    }
    heads.sort_by(|a, b| a.name.cmp(&b.name));
    heads
}

/// Brings every connected monitor in line with its settings: monitors whose
/// mode, mirroring or on/off state changed are set up again (they keep
/// their workspace); scale and position are changed in place by the caller.
pub fn reapply_outputs(state: &mut State) {
    let Backend::Udev(data) = &state.backend else {
        return;
    };
    let todo: Vec<_> = data
        .devices
        .iter()
        .flat_map(|(node, device)| {
            device.scanner.crtcs().map(move |(info, crtc)| {
                let output = device.surfaces.get(&crtc).map(|s| s.output.clone());
                (*node, info.clone(), crtc, output)
            })
        })
        .collect();
    for (node, info, crtc, current) in todo {
        let name = connector_name(&info);
        let cfg = state.output_config(&name);
        let Some(output) = current else {
            if !cfg.disable {
                info!("output {name}: enabling");
                connector_connected(state, node, info, crtc);
            }
            continue;
        };
        let mirror_now = state
            .mirrors
            .iter()
            .find(|m| m.output == output)
            .map(|m| m.source.clone());
        let mirror_wanted = cfg.mirror.clone().filter(|s| s != &name);
        let mode_wanted = pick_mode(&info, cfg.mode.as_deref()).map(WlMode::from);
        let changed =
            cfg.disable || mode_wanted != output.current_mode() || mirror_now != mirror_wanted;
        if !changed {
            continue;
        }
        let ws = state.output_ws(&output);
        connector_disconnected(state, node, crtc);
        if cfg.disable {
            info!("output {name}: disabled");
            continue;
        }
        info!("output {name}: setting up again");
        connector_connected(state, node, info, crtc);
        if let Some(new) = state.output_by_name(&name) {
            // Keep showing the same workspace, swapping with whoever has it.
            let mine = state.output_ws(&new);
            if let Some(other) = state
                .outputs
                .iter()
                .find(|o| **o != new && state.output_ws(o) == ws)
            {
                output_state(other).ws = mine;
            }
            output_state(&new).ws = ws;
        }
    }
}

fn crtc_of(data: &UdevData, output: &Output) -> Option<(crtc::Handle, DrmNode)> {
    let id = *output.user_data().get::<OutputId>()?;
    data.devices
        .get(&id.node)?
        .surfaces
        .contains_key(&id.crtc)
        .then_some((id.crtc, id.node))
}

pub fn gamma_size(data: &UdevData, output: &Output) -> Option<u32> {
    let (crtc, node) = crtc_of(data, output)?;
    let drm = data.devices.get(&node)?.manager.device();
    let len = drm.get_crtc(crtc).ok()?.gamma_length();
    (len > 1).then_some(len)
}

/// `ramp` holds the red, then green, then blue values; `None` is linear.
pub fn set_gamma(data: &UdevData, output: &Output, ramp: Option<&[u16]>) -> Result<(), String> {
    let (crtc, node) = crtc_of(data, output).ok_or("output is gone")?;
    let drm = data
        .devices
        .get(&node)
        .ok_or("gpu is gone")?
        .manager
        .device();
    let len = drm
        .get_crtc(crtc)
        .map_err(|e| e.to_string())?
        .gamma_length() as usize;
    let linear: Vec<u16>;
    let ramp = match ramp {
        Some(r) if r.len() == len * 3 => r,
        Some(r) => {
            return Err(format!(
                "gamma ramp of {} values, want {}",
                r.len(),
                len * 3
            ));
        }
        None => {
            linear = linear_ramp(len).repeat(3);
            &linear
        }
    };
    let (red, rest) = ramp.split_at(len);
    let (green, blue) = rest.split_at(len);
    drm.set_gamma(crtc, red, green, blue).map_err(|e| {
        warn!("set gamma on {}: {e}", output.name());
        e.to_string()
    })
}

/// Identity ramp of `len` steps over the full u16 range.
pub fn linear_ramp(len: usize) -> Vec<u16> {
    let last = len.saturating_sub(1).max(1) as u32;
    (0..len as u32)
        .map(|i| (i * u16::MAX as u32 / last) as u16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_ramp_spans_the_full_range() {
        let ramp = linear_ramp(256);
        assert_eq!(ramp.len(), 256);
        assert_eq!(ramp[0], 0);
        assert_eq!(ramp[255], u16::MAX);
        assert!(ramp.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn tiny_ramps_do_not_divide_by_zero() {
        assert_eq!(linear_ramp(1), [0]);
        assert!(linear_ramp(0).is_empty());
    }
}
