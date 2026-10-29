//! wlr-output-management: monitor settings from other programs (kanshi
//! profiles, wdisplays, wlr-randr, nwg-displays). A configuration a client
//! applies becomes a runtime override of `[[output]]` (state.rs) and takes
//! effect at once; reloading the config file drops the overrides.

use std::sync::{Arc, Mutex};

use smithay::reexports::{
    wayland_protocols_wlr::output_management::v1::server::{
        zwlr_output_configuration_head_v1::{self, ZwlrOutputConfigurationHeadV1},
        zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
        zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
        zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
        zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
    },
    wayland_server::{
        Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
        backend::ClientId, protocol::wl_output::Transform,
    },
};
use tracing::info;

use crate::{
    backend::{HeadInfo, ModeInfo},
    config::OutputConfig,
    state::State,
};

const VERSION: u32 = 3;

#[derive(Debug, Default)]
pub struct OutputManagement {
    managers: Vec<Manager>,
    /// Bumped whenever the heads change; configurations made for an older
    /// serial are cancelled.
    serial: u32,
    last: Vec<HeadInfo>,
}

#[derive(Debug)]
struct Manager {
    manager: ZwlrOutputManagerV1,
    heads: Vec<(ZwlrOutputHeadV1, Vec<ZwlrOutputModeV1>)>,
}

pub struct HeadData {
    name: String,
}

pub struct ModeData {
    mode: ModeInfo,
}

/// What a client asked for one head.
#[derive(Debug, Default, Clone)]
pub struct HeadChange {
    mode: Option<ModeInfo>,
    custom_mode: Option<(i32, i32, i32)>,
    position: Option<(i32, i32)>,
    scale: Option<f64>,
    transform: Option<Transform>,
}

#[derive(Debug, Default)]
pub struct ConfigData {
    serial: u32,
    used: bool,
    /// Head name → change (`None`: disable it).
    heads: Vec<(String, Option<Arc<Mutex<HeadChange>>>)>,
}

pub fn init(dh: &DisplayHandle) {
    dh.create_global::<State, ZwlrOutputManagerV1, _>(VERSION, ());
}

/// The `[[output]]` entry a head should get, or why the change is refused.
pub fn override_for(
    head: &HeadInfo,
    mut base: OutputConfig,
    change: Option<&HeadChange>,
) -> Result<OutputConfig, String> {
    base.name = head.name.clone();
    let Some(change) = change else {
        base.disable = true;
        return Ok(base);
    };
    base.disable = false;
    if change.transform.is_some_and(|t| t != Transform::Normal) {
        return Err(format!("{}: rotated monitors are not supported", head.name));
    }
    let mode = match (change.mode, change.custom_mode) {
        (Some(m), _) => Some((m.width, m.height, m.refresh)),
        (None, Some((w, h, r))) => {
            let offered = head
                .modes
                .iter()
                .any(|m| m.width == w && m.height == h && (r == 0 || (m.refresh - r).abs() < 1000));
            if !offered {
                return Err(format!("{}: custom mode {w}x{h} is not offered", head.name));
            }
            Some((w, h, r))
        }
        (None, None) => None,
    };
    if let Some((w, h, r)) = mode {
        base.mode = Some(if r > 0 {
            format!("{w}x{h}@{:.3}", r as f64 / 1000.0)
        } else {
            format!("{w}x{h}")
        });
    }
    if let Some((x, y)) = change.position {
        base.position = Some(format!("{x},{y}"));
    }
    if let Some(scale) = change.scale {
        if !(0.5..=4.0).contains(&scale) {
            return Err(format!("{}: scale {scale} is out of 0.5 … 4", head.name));
        }
        base.scale = Some(scale);
    }
    Ok(base)
}

impl State {
    fn send_head(
        &self,
        manager: &ZwlrOutputManagerV1,
        head: &HeadInfo,
    ) -> Option<(ZwlrOutputHeadV1, Vec<ZwlrOutputModeV1>)> {
        let client = manager.client()?;
        let dh = &self.display_handle;
        let version = manager.version();
        let resource = client
            .create_resource::<ZwlrOutputHeadV1, _, State>(
                dh,
                version,
                HeadData {
                    name: head.name.clone(),
                },
            )
            .ok()?;
        manager.head(&resource);
        resource.name(head.name.clone());
        resource.description(format!("{} {}", head.make, head.model).trim().to_string());
        resource.physical_size(head.physical_size.0, head.physical_size.1);
        let mut modes = Vec::new();
        for mode in &head.modes {
            let Ok(m) = client.create_resource::<ZwlrOutputModeV1, _, State>(
                dh,
                version,
                ModeData { mode: *mode },
            ) else {
                continue;
            };
            resource.mode(&m);
            m.size(mode.width, mode.height);
            if mode.refresh > 0 {
                m.refresh(mode.refresh);
            }
            if mode.preferred {
                m.preferred();
            }
            modes.push(m);
        }
        resource.enabled(i32::from(head.enabled));
        if head.enabled {
            if let Some(m) = head.current_mode.and_then(|i| modes.get(i)) {
                resource.current_mode(m);
            }
            resource.position(head.position.0, head.position.1);
            resource.transform(Transform::Normal);
            resource.scale(head.scale);
        }
        if version >= 2 {
            if !head.make.is_empty() {
                resource.make(head.make.clone());
            }
            resource.model(head.model.clone());
        }
        Some((resource, modes))
    }

    /// Tells clients about changed monitors; runs after every event-loop pass.
    pub fn output_management_refresh(&mut self) {
        if self.output_management.managers.is_empty() {
            return;
        }
        let heads = crate::backend::heads(self);
        if heads == self.output_management.last {
            return;
        }
        let old = std::mem::replace(&mut self.output_management.last, heads.clone());
        self.output_management.serial = self.output_management.serial.wrapping_add(1);
        let serial = self.output_management.serial;
        let mut managers = std::mem::take(&mut self.output_management.managers);
        for m in &mut managers {
            // Changed or gone heads are finished, changed and new ones sent.
            m.heads.retain(|(head, modes)| {
                let name = &head
                    .data::<HeadData>()
                    .map(|d| d.name.clone())
                    .unwrap_or_default();
                let same =
                    old.iter().find(|h| &h.name == name) == heads.iter().find(|h| &h.name == name);
                if !same {
                    for mode in modes {
                        mode.finished();
                    }
                    head.finished();
                }
                same
            });
            for head in heads
                .iter()
                .filter(|h| old.iter().find(|o| o.name == h.name) != Some(*h))
            {
                if let Some(sent) = self.send_head(&m.manager, head) {
                    m.heads.push(sent);
                }
            }
            m.manager.done(serial);
        }
        self.output_management.managers = managers;
    }

    fn apply_output_configuration(&mut self, data: &ConfigData, test: bool) -> Result<(), String> {
        let heads = crate::backend::heads(self);
        let mut overrides = Vec::new();
        for (name, change) in &data.heads {
            let head = heads
                .iter()
                .find(|h| &h.name == name)
                .ok_or_else(|| format!("{name} is gone"))?;
            let change = change
                .as_ref()
                .map(|c| c.lock().map(|c| c.clone()).unwrap_or_default());
            overrides.push(override_for(
                head,
                self.output_config(name),
                change.as_ref(),
            )?);
        }
        let any_on = heads
            .iter()
            .any(|h| match overrides.iter().find(|o| o.name == h.name) {
                Some(o) => !o.disable,
                None => h.enabled,
            });
        if !any_on {
            return Err("that would turn every monitor off".into());
        }
        if test {
            return Ok(());
        }
        for o in overrides {
            info!("output management: {} → {o:?}", o.name);
            self.output_overrides.retain(|old| old.name != o.name);
            self.output_overrides.push(o);
        }
        crate::backend::reapply_outputs(self);
        Ok(())
    }
}

impl GlobalDispatch<ZwlrOutputManagerV1, ()> for State {
    fn bind(
        state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrOutputManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        let manager = data_init.init(resource, ());
        if state.output_management.managers.is_empty() {
            state.output_management.last = crate::backend::heads(state);
        }
        let heads = state
            .output_management
            .last
            .iter()
            .filter_map(|h| state.send_head(&manager, h))
            .collect();
        manager.done(state.output_management.serial);
        state
            .output_management
            .managers
            .push(Manager { manager, heads });
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for State {
    fn request(
        state: &mut State,
        _client: &Client,
        manager: &ZwlrOutputManagerV1,
        request: zwlr_output_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            zwlr_output_manager_v1::Request::CreateConfiguration { id, serial } => {
                data_init.init(
                    id,
                    Mutex::new(ConfigData {
                        serial,
                        ..Default::default()
                    }),
                );
            }
            zwlr_output_manager_v1::Request::Stop => {
                state
                    .output_management
                    .managers
                    .retain(|m| &m.manager != manager);
                manager.finished();
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut State, _client: ClientId, manager: &ZwlrOutputManagerV1, _data: &()) {
        state
            .output_management
            .managers
            .retain(|m| &m.manager != manager);
    }
}

impl Dispatch<ZwlrOutputHeadV1, HeadData> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        _head: &ZwlrOutputHeadV1,
        _request: zwlr_output_head_v1::Request,
        _data: &HeadData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
    }

    fn destroyed(state: &mut State, _client: ClientId, head: &ZwlrOutputHeadV1, _data: &HeadData) {
        for m in &mut state.output_management.managers {
            m.heads.retain(|(h, _)| h != head);
        }
    }
}

impl Dispatch<ZwlrOutputModeV1, ModeData> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        _mode: &ZwlrOutputModeV1,
        _request: zwlr_output_mode_v1::Request,
        _data: &ModeData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, Mutex<ConfigData>> for State {
    fn request(
        state: &mut State,
        _client: &Client,
        config: &ZwlrOutputConfigurationV1,
        request: zwlr_output_configuration_v1::Request,
        data: &Mutex<ConfigData>,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use zwlr_output_configuration_v1::{Error, Request};
        let Ok(mut cfg) = data.lock() else {
            return;
        };
        let head_name = |head: &ZwlrOutputHeadV1| head.data::<HeadData>().map(|d| d.name.clone());
        match request {
            Request::EnableHead { id, head } => {
                let change = Arc::new(Mutex::new(HeadChange::default()));
                data_init.init(id, change.clone());
                let Some(name) = head_name(&head) else {
                    return;
                };
                if cfg.heads.iter().any(|(n, _)| *n == name) {
                    config.post_error(Error::AlreadyConfiguredHead, format!("{name} twice"));
                    return;
                }
                cfg.heads.push((name, Some(change)));
            }
            Request::DisableHead { head } => {
                let Some(name) = head_name(&head) else {
                    return;
                };
                if cfg.heads.iter().any(|(n, _)| *n == name) {
                    config.post_error(Error::AlreadyConfiguredHead, format!("{name} twice"));
                    return;
                }
                cfg.heads.push((name, None));
            }
            Request::Apply | Request::Test => {
                if cfg.used {
                    config.post_error(Error::AlreadyUsed, "configuration already used");
                    return;
                }
                cfg.used = true;
                if cfg.serial != state.output_management.serial {
                    config.cancelled();
                    return;
                }
                let test = matches!(request, Request::Test);
                match state.apply_output_configuration(&cfg, test) {
                    Ok(()) => config.succeeded(),
                    Err(err) => {
                        info!("output configuration refused: {err}");
                        config.failed();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, Arc<Mutex<HeadChange>>> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        resource: &ZwlrOutputConfigurationHeadV1,
        request: zwlr_output_configuration_head_v1::Request,
        data: &Arc<Mutex<HeadChange>>,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use zwlr_output_configuration_head_v1::{Error, Request};
        let Ok(mut change) = data.lock() else {
            return;
        };
        match request {
            Request::SetMode { mode } => match mode.data::<ModeData>() {
                Some(m) => change.mode = Some(m.mode),
                None => resource.post_error(Error::InvalidMode, "unknown mode"),
            },
            Request::SetCustomMode {
                width,
                height,
                refresh,
            } => {
                if width <= 0 || height <= 0 || refresh < 0 {
                    resource.post_error(Error::InvalidCustomMode, "bad custom mode");
                    return;
                }
                change.custom_mode = Some((width, height, refresh));
            }
            Request::SetPosition { x, y } => change.position = Some((x, y)),
            Request::SetScale { scale } => {
                if scale <= 0.0 {
                    resource.post_error(Error::InvalidScale, "scale must be positive");
                    return;
                }
                change.scale = Some(scale);
            }
            Request::SetTransform { transform } => match transform {
                WEnum::Value(t) => change.transform = Some(t),
                WEnum::Unknown(_) => {
                    resource.post_error(Error::InvalidTransform, "unknown transform")
                }
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head() -> HeadInfo {
        HeadInfo {
            name: "DP-1".into(),
            make: String::new(),
            model: "DP-1".into(),
            physical_size: (600, 340),
            modes: vec![
                ModeInfo {
                    width: 2560,
                    height: 1440,
                    refresh: 165_000,
                    preferred: true,
                },
                ModeInfo {
                    width: 1920,
                    height: 1080,
                    refresh: 60_000,
                    preferred: false,
                },
            ],
            current_mode: Some(0),
            enabled: true,
            position: (0, 0),
            scale: 1.0,
        }
    }

    #[test]
    fn disabling_keeps_other_settings() {
        let base = OutputConfig {
            scale: Some(1.5),
            ..Default::default()
        };
        let o = override_for(&head(), base, None).unwrap();
        assert!(o.disable);
        assert_eq!(o.scale, Some(1.5));
        assert_eq!(o.name, "DP-1");
    }

    #[test]
    fn mode_position_and_scale_become_config_values() {
        let change = HeadChange {
            mode: Some(head().modes[1]),
            position: Some((-1920, 0)),
            scale: Some(1.25),
            ..Default::default()
        };
        let o = override_for(&head(), OutputConfig::default(), Some(&change)).unwrap();
        assert!(!o.disable);
        assert_eq!(o.mode.as_deref(), Some("1920x1080@60.000"));
        assert_eq!(o.position(), Some((-1920, 0)));
        assert_eq!(o.scale, Some(1.25));
    }

    #[test]
    fn unsupported_requests_are_refused() {
        let rotated = HeadChange {
            transform: Some(Transform::_90),
            ..Default::default()
        };
        assert!(override_for(&head(), OutputConfig::default(), Some(&rotated)).is_err());
        let odd_mode = HeadChange {
            custom_mode: Some((1234, 567, 60_000)),
            ..Default::default()
        };
        assert!(override_for(&head(), OutputConfig::default(), Some(&odd_mode)).is_err());
        let huge = HeadChange {
            scale: Some(9.0),
            ..Default::default()
        };
        assert!(override_for(&head(), OutputConfig::default(), Some(&huge)).is_err());
    }

    #[test]
    fn custom_mode_matching_an_offered_one_is_fine() {
        let change = HeadChange {
            custom_mode: Some((2560, 1440, 164_999)),
            ..Default::default()
        };
        let o = override_for(&head(), OutputConfig::default(), Some(&change)).unwrap();
        assert_eq!(o.mode.as_deref(), Some("2560x1440@164.999"));
    }
}
