// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Native backend: DRM/KMS outputs, libinput devices and a libseat session.
//! (Kept as one file on purpose: device, output and frame-pacing code share
//! private types and read best together.)
//! Works with several GPUs (e.g. AMD iGPU + NVIDIA dGPU laptops): rendering
//! happens on the primary GPU and is copied to outputs on the other one.

use std::{collections::HashMap, path::Path, time::Duration};

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            dmabuf::Dmabuf,
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmEventTime as DrmTime, DrmNode,
            NodeType,
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLDevice, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            ImportDma, ImportMemWl,
            gles::{GlesError, GlesFrame, GlesRenderer},
            multigpu::{GpuManager, MultiRenderer, gbm::GbmGlesBackend},
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    desktop::utils::OutputPresentationFeedback,
    output::{Mode as WlMode, Output, PhysicalProperties, Scale},
    reexports::{
        calloop::{
            EventLoop, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::{
            Device as _,
            control::{ModeTypeFlags, connector, crtc},
        },
        input::{DeviceCapability, Libinput},
        rustix::fs::OFlags,
        wayland_protocols::wp::presentation_time::server::wp_presentation_feedback::Kind,
        wayland_server::{Display, protocol::wl_surface::WlSurface},
    },
    utils::DeviceFd,
    wayland::{
        dmabuf::{DmabufFeedbackBuilder, DmabufState},
        presentation::Refresh,
    },
};
use smithay_drm_extras::drm_scanner::{DrmScanEvent, DrmScanner};
use tracing::{error, info, warn};

mod outputs;

pub use outputs::{gamma_size, heads, reapply_outputs, set_gamma};

use super::Backend;
use crate::{
    config::{Config, Input as InputConfig},
    render::{
        PzElement, PzRenderer, mirror_scale, output_elements, post_render, presentation_feedback,
        scaled_output_elements,
    },
    state::{RedrawState, State, output_state},
};

type GpuApi = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;
pub type UdevRenderer<'a> = MultiRenderer<'a, 'a, GpuApi, GpuApi>;
type OutputManager = DrmOutputManager<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    FrameFeedback,
    DrmDeviceFd,
>;
type DrmOut = DrmOutput<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    FrameFeedback,
    DrmDeviceFd,
>;
/// Presentation feedback travelling with a queued frame (none for mirrors).
type FrameFeedback = Option<OutputPresentationFeedback>;

/// How long a removed output's global stays disabled before it is destroyed.
const OUTPUT_GLOBAL_GRACE: Duration = Duration::from_secs(5);

/// 8-bit formats only: 10-bit trips up some drivers and gains little here.
const COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Abgr8888];

impl<'a> PzRenderer for UdevRenderer<'a> {
    fn gles(&mut self) -> &mut GlesRenderer {
        self.as_mut()
    }

    fn gles_frame<'b, 'frame, 'buffer>(
        frame: &'b mut Self::Frame<'frame, 'buffer>,
    ) -> &'b mut GlesFrame<'frame, 'buffer>
    where
        'buffer: 'frame,
        Self: 'frame,
    {
        frame.as_mut()
    }

    fn wrap_error(err: GlesError) -> Self::Error {
        smithay::backend::renderer::multigpu::Error::Render(err)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutputId {
    node: DrmNode,
    crtc: crtc::Handle,
}

struct Surface {
    output: Output,
    drm_output: DrmOut,
    vblank_timer: Option<RegistrationToken>,
}

struct Device {
    manager: OutputManager,
    scanner: DrmScanner,
    render_node: Option<DrmNode>,
    surfaces: HashMap<crtc::Handle, Surface>,
    token: RegistrationToken,
}

pub struct UdevData {
    session: LibSeatSession,
    primary_gpu: DrmNode,
    gpus: GpuManager<GpuApi>,
    devices: HashMap<DrmNode, Device>,
    libinput: Libinput,
    input_devices: Vec<smithay::reexports::input::Device>,
}

impl UdevData {
    pub fn seat_name(&self) -> String {
        self.session.seat()
    }

    pub fn early_import(&mut self, surface: &WlSurface) {
        if let Err(err) = self.gpus.early_import(self.primary_gpu, surface) {
            tracing::trace!("early import failed: {err}");
        }
    }

    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> bool {
        let ok = self
            .gpus
            .single_renderer(&self.primary_gpu)
            .and_then(|mut r| r.import_dmabuf(dmabuf, None))
            .is_ok();
        if ok {
            dmabuf.set_node(self.primary_gpu);
        }
        ok
    }

    pub fn switch_vt(&mut self, vt: i32) {
        if let Err(err) = self.session.change_vt(vt) {
            warn!("VT switch to {vt} failed: {err}");
        }
    }

    pub fn configure_devices(&mut self, input: &InputConfig) {
        for device in &mut self.input_devices {
            configure_device(device, input);
        }
    }
}

fn configure_device(device: &mut smithay::reexports::input::Device, input: &InputConfig) {
    if !device.has_capability(DeviceCapability::Pointer) {
        return;
    }
    let touchpad = device.config_tap_finger_count() > 0;
    if touchpad {
        let _ = device.config_tap_set_enabled(input.tap_to_click);
        let _ = device.config_tap_set_drag_enabled(input.tap_and_drag);
        let _ = device.config_dwt_set_enabled(input.disable_while_typing);
    }
    if device.config_scroll_has_natural_scroll() {
        let natural = if touchpad {
            input.natural_scroll
        } else {
            input.mouse_natural_scroll
        };
        let _ = device.config_scroll_set_natural_scroll_enabled(natural);
    }
    if device.config_accel_is_available() {
        let _ = device.config_accel_set_speed(input.accel_speed.clamp(-1.0, 1.0));
    }
}

pub fn init(event_loop: &mut EventLoop<'static, State>, config: Config) -> anyhow::Result<State> {
    let display: Display<State> = Display::new()?;
    let (session, notifier) = LibSeatSession::new()
        .map_err(|e| anyhow::anyhow!("no seat session (start pzwm from a TTY): {e}"))?;

    let primary_gpu = std::env::var("PZWM_DRM_DEVICE")
        .ok()
        .and_then(|path| DrmNode::from_path(path).ok())
        .or_else(|| {
            primary_gpu(session.seat()).ok().flatten().and_then(|p| {
                DrmNode::from_path(p)
                    .ok()?
                    .node_with_type(NodeType::Render)?
                    .ok()
            })
        })
        .or_else(|| {
            all_gpus(session.seat())
                .ok()?
                .into_iter()
                .find_map(|p| DrmNode::from_path(p).ok())
        })
        .ok_or_else(|| anyhow::anyhow!("no GPU found"))?;
    info!("primary GPU: {primary_gpu}");

    let gpus =
        GpuManager::new(GpuApi::default()).map_err(|e| anyhow::anyhow!("gpu manager: {e}"))?;
    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&session.seat())
        .map_err(|_| anyhow::anyhow!("libinput could not assign seat"))?;

    let data = UdevData {
        session,
        primary_gpu,
        gpus,
        devices: HashMap::new(),
        libinput: libinput.clone(),
        input_devices: Vec::new(),
    };
    let mut state = State::new(
        display,
        event_loop.handle(),
        event_loop.get_signal(),
        Backend::Udev(Box::new(data)),
        config,
    )?;

    let udev = UdevBackend::new(state.backend.seat_name())?;
    let primary_node = primary_gpu
        .node_with_type(NodeType::Primary)
        .and_then(Result::ok);
    let mut devices: Vec<(DrmNode, std::path::PathBuf)> = udev
        .device_list()
        .filter_map(|(id, path)| Some((DrmNode::from_dev_id(id).ok()?, path.to_path_buf())))
        .collect();
    // The primary GPU first, so display-only GPUs can borrow its renderer.
    devices.sort_by_key(|(node, _)| Some(*node) != primary_node && *node != primary_gpu);
    for (node, path) in devices {
        if let Err(err) = device_added(&mut state, node, &path) {
            warn!("skipping {}: {err}", path.display());
        }
    }

    {
        let Backend::Udev(data) = &mut state.backend else {
            unreachable!()
        };
        let renderer = data
            .gpus
            .single_renderer(&primary_gpu)
            .map_err(|e| anyhow::anyhow!("primary renderer: {e}"))?;
        let shm_formats = renderer.shm_formats();
        let dmabuf_formats = renderer.dmabuf_formats();
        drop(renderer);
        state.shm_state.update_formats(shm_formats);
        let feedback = DmabufFeedbackBuilder::new(primary_gpu.dev_id(), dmabuf_formats)
            .build()
            .map_err(|e| anyhow::anyhow!("dmabuf feedback: {e}"))?;
        let mut dmabuf_state = DmabufState::new();
        let global = dmabuf_state
            .create_global_with_default_feedback::<State>(&state.display_handle, &feedback);
        state.dmabuf_state = Some((dmabuf_state, global));
    }

    event_loop
        .handle()
        .insert_source(udev, |event, _, state| match event {
            UdevEvent::Added { device_id, path } => {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    if let Err(err) = device_added(state, node, &path) {
                        warn!("skipping {}: {err}", path.display());
                    }
                }
            }
            UdevEvent::Changed { device_id } => {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    device_changed(state, node);
                }
            }
            UdevEvent::Removed { device_id } => {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    device_removed(state, node);
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("udev source: {e}"))?;

    event_loop
        .handle()
        .insert_source(
            LibinputInputBackend::new(libinput),
            |mut event, _, state| {
                if let InputEvent::DeviceAdded { device } = &mut event {
                    let input = state.config.input.clone();
                    configure_device(device, &input);
                    if let Backend::Udev(data) = &mut state.backend {
                        data.input_devices.push(device.clone());
                    }
                } else if let InputEvent::DeviceRemoved { device } = &event {
                    if let Backend::Udev(data) = &mut state.backend {
                        data.input_devices.retain(|d| d != device);
                    }
                }
                state.process_input_event(event);
            },
        )
        .map_err(|e| anyhow::anyhow!("libinput source: {e}"))?;

    event_loop
        .handle()
        .insert_source(notifier, |event, _, state| match event {
            SessionEvent::PauseSession => {
                info!("session paused (VT switch)");
                if let Backend::Udev(data) = &mut state.backend {
                    data.libinput.suspend();
                    for device in data.devices.values_mut() {
                        device.manager.pause();
                    }
                }
            }
            SessionEvent::ActivateSession => {
                info!("session resumed");
                if let Backend::Udev(data) = &mut state.backend {
                    if data.libinput.resume().is_err() {
                        error!("failed to resume libinput");
                    }
                    for device in data.devices.values_mut() {
                        if let Err(err) = device.manager.activate(false) {
                            error!("failed to reactivate DRM device: {err}");
                        }
                    }
                }
                for output in state.outputs.clone() {
                    output_state(&output).redraw = RedrawState::Idle;
                }
                state.queue_redraw_all();
            }
        })
        .map_err(|e| anyhow::anyhow!("session source: {e}"))?;

    state.queue_redraw_all();
    Ok(state)
}

fn device_added(state: &mut State, node: DrmNode, path: &Path) -> anyhow::Result<()> {
    let Backend::Udev(data) = &mut state.backend else {
        return Ok(());
    };
    let fd = data.session.open(
        path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd)?;

    let token = state
        .loop_handle
        .insert_source(notifier, move |event, metadata, state| match event {
            DrmEvent::VBlank(crtc) => frame_finished(state, node, crtc, metadata.take()),
            DrmEvent::Error(err) => error!("DRM error: {err}"),
        })
        .map_err(|e| anyhow::anyhow!("drm source: {e}"))?;

    // SAFETY: the GBM device stays alive as long as the EGL display created from it.
    let render_node = unsafe { EGLDisplay::new(gbm.clone()) }
        .ok()
        .and_then(|display| EGLDevice::device_for_display(&display).ok())
        .filter(|dev| !dev.is_software())
        .map(|dev| dev.try_get_render_node().ok().flatten().unwrap_or(node))
        .and_then(|render_node| {
            data.gpus
                .as_mut()
                .add_node(render_node, gbm.clone())
                .inspect_err(|err| warn!("cannot render on {render_node}: {err}"))
                .ok()
                .map(|_| render_node)
        });

    let allocator = match render_node {
        Some(_) => GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        ),
        None => data
            .devices
            .values()
            .find(|d| d.render_node == Some(data.primary_gpu))
            .map(|d| d.manager.allocator().clone())
            .ok_or_else(|| anyhow::anyhow!("display-only GPU without a primary GPU"))?,
    };
    let exporter = GbmFramebufferExporter::new(gbm.clone(), render_node);
    let mut renderer = data
        .gpus
        .single_renderer(&render_node.unwrap_or(data.primary_gpu))
        .map_err(|e| anyhow::anyhow!("renderer: {e}"))?;
    let render_formats: FormatSet = renderer
        .as_mut()
        .egl_context()
        .dmabuf_render_formats()
        .iter()
        .filter(|f| {
            render_node.is_some() || f.modifier == smithay::backend::allocator::Modifier::Linear
        })
        .copied()
        .collect();
    drop(renderer);

    let manager = DrmOutputManager::new(
        drm,
        allocator,
        exporter,
        Some(gbm),
        COLOR_FORMATS,
        render_formats,
    );
    data.devices.insert(
        node,
        Device {
            manager,
            scanner: DrmScanner::new(),
            render_node,
            surfaces: HashMap::new(),
            token,
        },
    );
    device_changed(state, node);
    Ok(())
}

fn device_changed(state: &mut State, node: DrmNode) {
    let Backend::Udev(data) = &mut state.backend else {
        return;
    };
    let Some(device) = data.devices.get_mut(&node) else {
        return;
    };
    let events = match device.scanner.scan_connectors(device.manager.device()) {
        Ok(result) => result.into_iter().collect::<Vec<_>>(),
        Err(err) => {
            warn!("connector scan failed: {err}");
            return;
        }
    };
    for event in events {
        match event {
            DrmScanEvent::Connected {
                connector,
                crtc: Some(crtc),
            } => connector_connected(state, node, connector, crtc),
            DrmScanEvent::Disconnected {
                crtc: Some(crtc), ..
            } => connector_disconnected(state, node, crtc),
            _ => {}
        }
    }
    state.relayout_outputs();
}

fn device_removed(state: &mut State, node: DrmNode) {
    let crtcs: Vec<crtc::Handle> = match &state.backend {
        Backend::Udev(data) => data
            .devices
            .get(&node)
            .map(|d| d.surfaces.keys().copied().collect())
            .unwrap_or_default(),
        _ => return,
    };
    for crtc in crtcs {
        connector_disconnected(state, node, crtc);
    }
    let Backend::Udev(data) = &mut state.backend else {
        return;
    };
    if let Some(device) = data.devices.remove(&node) {
        if let Some(render_node) = device.render_node {
            data.gpus.as_mut().remove_node(&render_node);
        }
        state.loop_handle.remove(device.token);
    }
    state.relayout_outputs();
}

/// Parses "2560x1440@165" / "1920x1080".
fn parse_mode(s: &str) -> Option<(u16, u16, Option<f64>)> {
    let (size, refresh) = match s.split_once('@') {
        Some((size, r)) => (size, r.trim().parse::<f64>().ok()),
        None => (s, None),
    };
    let (w, h) = size.trim().split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?, refresh))
}

fn pick_mode(
    connector: &connector::Info,
    wanted: Option<&str>,
) -> Option<smithay::reexports::drm::control::Mode> {
    let modes = connector.modes();
    if let Some((w, h, refresh)) = wanted.and_then(parse_mode) {
        let best = modes.iter().filter(|m| m.size() == (w, h)).min_by(|a, b| {
            let target = refresh.unwrap_or(f64::MAX);
            let da = (a.vrefresh() as f64 - target).abs();
            let db = (b.vrefresh() as f64 - target).abs();
            da.total_cmp(&db)
        });
        if let Some(mode) = best {
            return Some(*mode);
        }
        warn!(
            "mode {w}x{h} not offered by {:?}, using preferred",
            connector.interface()
        );
    }
    modes
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| modes.first())
        .copied()
}

fn connector_connected(
    state: &mut State,
    node: DrmNode,
    connector: connector::Info,
    crtc: crtc::Handle,
) {
    let name = format!(
        "{}-{}",
        connector.interface().as_str(),
        connector.interface_id()
    );
    let output_config = state.output_config(&name);
    let scale = state.output_scale(&name);
    if output_config.disable {
        info!("output {name} disabled in config");
        return;
    }
    let Backend::Udev(data) = &mut state.backend else {
        return;
    };
    let primary_gpu = data.primary_gpu;
    let Some(device) = data.devices.get_mut(&node) else {
        return;
    };
    let Some(drm_mode) = pick_mode(&connector, output_config.mode.as_deref()) else {
        warn!("{name}: no usable mode");
        return;
    };
    let wl_mode = WlMode::from(drm_mode);
    let (phys_w, phys_h) = connector.size().unwrap_or((0, 0));
    let output = Output::new(
        name.clone(),
        PhysicalProperties {
            size: (phys_w as i32, phys_h as i32).into(),
            subpixel: connector.subpixel().into(),
            make: "Unknown".into(),
            model: name.clone(),
        },
    );
    let mirror_of = output_config
        .mirror
        .clone()
        .filter(|source| source != &name);
    // A mirror is invisible to clients: no bar, no windows of its own.
    if mirror_of.is_none() {
        let global = output.create_global::<State>(&state.display_handle);
        output
            .user_data()
            .insert_if_missing(|| OutputGlobal(global));
    }
    output.set_preferred(wl_mode);
    output.change_current_state(Some(wl_mode), None, Some(Scale::Fractional(scale)), None);
    output
        .user_data()
        .insert_if_missing(|| OutputId { node, crtc });

    let drm = device.manager.device();
    let mut planes = match drm.planes(&crtc) {
        Ok(planes) => planes,
        Err(err) => {
            warn!("{name}: cannot query planes: {err}");
            return;
        }
    };
    let nvidia = drm
        .get_driver()
        .map(|d| d.name().to_string_lossy().to_lowercase().contains("nvidia"))
        .unwrap_or(false);
    if nvidia {
        // Overlay planes misbehave on the NVIDIA driver.
        planes.overlay.clear();
    }

    let render_node = device.render_node.unwrap_or(primary_gpu);
    let mut renderer = match data.gpus.single_renderer(&render_node) {
        Ok(r) => r,
        Err(err) => {
            warn!("{name}: no renderer: {err}");
            return;
        }
    };
    let drm_output = match device
        .manager
        .initialize_output::<_, PzElement<UdevRenderer<'_>>>(
            crtc,
            drm_mode,
            &[connector.handle()],
            &output,
            Some(planes),
            &mut renderer,
            &DrmOutputRenderElements::default(),
        ) {
        Ok(o) => o,
        Err(err) => {
            warn!("{name}: output init failed: {err}");
            return;
        }
    };
    drop(renderer);
    device.surfaces.insert(
        crtc,
        Surface {
            output: output.clone(),
            drm_output,
            vblank_timer: None,
        },
    );

    if let Some(source) = mirror_of {
        info!(
            "output {name}: {}x{} mirrors {source}",
            wl_mode.size.w, wl_mode.size.h
        );
        state.mirrors.push(crate::state::Mirror {
            output: output.clone(),
            source,
        });
        queue_redraw(state, &output);
        return;
    }

    let position = output_config.position().unwrap_or_else(|| {
        let right = state
            .outputs
            .iter()
            .map(|o| {
                let g = state.output_geometry(o);
                g.loc.x + g.size.w
            })
            .max()
            .unwrap_or(0);
        (right, 0)
    });
    output.change_current_state(None, None, None, Some(position.into()));
    info!(
        "output {name}: {}x{}@{:.0}Hz scale {scale} at {position:?}",
        wl_mode.size.w,
        wl_mode.size.h,
        wl_mode.refresh as f64 / 1000.0
    );

    // A fresh output shows the first workspace not already on screen.
    let used: Vec<usize> = state.outputs.iter().map(|o| state.output_ws(o)).collect();
    output_state(&output).ws = (0..crate::canvas::WORKSPACES)
        .find(|w| !used.contains(w))
        .unwrap_or(0);
    if state.outputs.is_empty() {
        let g = state.output_geometry(&output);
        state.cursor = (g.loc + g.size.downscale(2).to_point()).to_f64();
    }
    state.outputs.push(output.clone());
    state.queue_redraw(&output);
}

fn connector_disconnected(state: &mut State, node: DrmNode, crtc: crtc::Handle) {
    let Backend::Udev(data) = &mut state.backend else {
        return;
    };
    let Some(device) = data.devices.get_mut(&node) else {
        return;
    };
    if let Some(surface) = device.surfaces.remove(&crtc) {
        info!("output {} disconnected", surface.output.name());
        if let Some(token) = surface.vblank_timer {
            state.loop_handle.remove(token);
        }
        state.outputs.retain(|o| o != &surface.output);
        state.mirrors.retain(|m| m.output != surface.output);
        state.mirror_caches.remove(&surface.output.name());
        crate::screencopy::fail_output(state, &surface.output);
        remove_output_global(state, &surface.output);
        state.render_cache_drop_output(&surface.output);
    }
}

/// The `wl_output` global of an output, kept so it can be withdrawn when the
/// monitor goes away (otherwise clients keep seeing, and capturing, a ghost).
struct OutputGlobal(smithay::reexports::wayland_server::backend::GlobalId);

/// Disables the global now and destroys it a little later, giving clients
/// time to see the removal before the object goes away.
fn remove_output_global(state: &mut State, output: &Output) {
    let Some(OutputGlobal(id)) = output.user_data().get::<OutputGlobal>() else {
        return;
    };
    let id = id.clone();
    state.display_handle.disable_global::<State>(id.clone());
    let _ = state.loop_handle.insert_source(
        Timer::from_duration(OUTPUT_GLOBAL_GRACE),
        move |_, _, state| {
            state.display_handle.remove_global::<State>(id.clone());
            TimeoutAction::Drop
        },
    );
}

fn find_surface<'a>(
    devices: &'a mut HashMap<DrmNode, Device>,
    output: &Output,
) -> Option<(&'a mut Device, crtc::Handle)> {
    let id = *output.user_data().get::<OutputId>()?;
    let device = devices.get_mut(&id.node)?;
    device
        .surfaces
        .contains_key(&id.crtc)
        .then_some((device, id.crtc))
}

pub fn queue_redraw(state: &mut State, output: &Output) {
    let mut os = output_state(output);
    match os.redraw {
        RedrawState::Idle => {
            os.redraw = RedrawState::Queued;
            drop(os);
            let output = output.clone();
            state
                .loop_handle
                .insert_idle(move |state| render(state, &output));
        }
        RedrawState::WaitingForVblank { .. } => {
            os.redraw = RedrawState::WaitingForVblank { queued: true }
        }
        RedrawState::Queued => {}
    }
}

fn frame_duration(output: &Output) -> Duration {
    let refresh = output
        .current_mode()
        .map(|m| m.refresh)
        .filter(|r| *r > 0)
        .unwrap_or(60_000);
    Duration::from_secs_f64(1000.0 / refresh as f64)
}

fn render(state: &mut State, output: &Output) {
    if output_state(output).redraw != RedrawState::Queued {
        return;
    }
    let Backend::Udev(mut data) = std::mem::replace(&mut state.backend, Backend::Taken) else {
        return;
    };
    let rendered = render_inner(state, &mut data, output);
    state.backend = Backend::Udev(data);

    match rendered {
        Some(true) => output_state(output).redraw = RedrawState::WaitingForVblank { queued: false },
        Some(false) => {
            // Nothing changed: pace clients with a timer instead of a flip.
            output_state(output).redraw = RedrawState::WaitingForVblank { queued: false };
            let out = output.clone();
            let token = state
                .loop_handle
                .insert_source(
                    Timer::from_duration(frame_duration(output)),
                    move |_, _, state| {
                        vblank(state, &out, false);
                        TimeoutAction::Drop
                    },
                )
                .ok();
            if let Backend::Udev(data) = &mut state.backend {
                if let Some((device, crtc)) = find_surface(&mut data.devices, output) {
                    if let Some(s) = device.surfaces.get_mut(&crtc) {
                        s.vblank_timer = token;
                    }
                }
            }
        }
        // Session inactive or a hard error: wait for the next activation.
        None => output_state(output).redraw = RedrawState::Idle,
    }
    // Frame callbacks and captures belong to the source output.
    if state.mirror_source(output).is_none() {
        post_render(state, output);
    }
    // Through State so the output's mirrors repaint too.
    if std::mem::take(&mut output_state(output).redraw_pending) {
        state.queue_redraw(output);
    }
    state.popups.cleanup();
    let _ = state.display_handle.flush_clients();
}

/// Turns the screens off (planes cleared, DPMS off) or lets the next
/// frame turn them on again with a full repaint.
pub fn set_screens_power(state: &mut State, on: bool) {
    let Backend::Udev(data) = &mut state.backend else {
        return;
    };
    let mut timers = Vec::new();
    for device in data.devices.values_mut() {
        for surface in device.surfaces.values_mut() {
            if on {
                surface.drm_output.reset_buffers();
            } else {
                if let Err(err) = surface.drm_output.with_compositor(|c| c.clear()) {
                    warn!("screen off on {}: {err}", surface.output.name());
                }
                timers.extend(surface.vblank_timer.take());
                output_state(&surface.output).redraw = RedrawState::Idle;
            }
        }
    }
    for token in timers {
        state.loop_handle.remove(token);
    }
}

/// Some(true) when a frame was queued, Some(false) when there was no damage.
fn render_inner(state: &mut State, data: &mut UdevData, output: &Output) -> Option<bool> {
    if !data.session.is_active() || state.idle.screens_off {
        return None;
    }
    let primary_gpu = data.primary_gpu;
    let (device, crtc) = find_surface(&mut data.devices, output)?;
    let render_node = device.render_node.unwrap_or(primary_gpu);
    let surface = device.surfaces.get_mut(&crtc)?;
    let mut renderer = if render_node == primary_gpu {
        data.gpus.single_renderer(&render_node)
    } else {
        let format = surface.drm_output.format();
        data.gpus.renderer(&primary_gpu, &render_node, format)
    }
    .inspect_err(|err| warn!("no renderer for {}: {err}", output.name()))
    .ok()?;

    let (elements, clear) = match state.mirror_source(output) {
        None => output_elements(state, &mut renderer, output),
        Some(Some(source)) => {
            let size = output.current_mode().map(|m| m.size).unwrap_or_default();
            let os = mirror_scale(size, state.output_geometry(&source).size);
            // Surfaces take their size from the output's scale, not from
            // `os`: without this the wallpaper stays at 1:1 and leaves bars.
            if output.current_scale().fractional_scale() != os {
                output.change_current_state(None, None, Some(Scale::Fractional(os)), None);
            }
            let own = state
                .mirror_caches
                .remove(&output.name())
                .unwrap_or_default();
            let source_cache = std::mem::replace(&mut state.render_cache, own);
            let drawn = scaled_output_elements(state, &mut renderer, &source, os);
            let own = std::mem::replace(&mut state.render_cache, source_cache);
            state.mirror_caches.insert(output.name(), own);
            drawn
        }
        // The source is unplugged: black.
        Some(None) => (Vec::new(), [0.0, 0.0, 0.0, 1.0]),
    };
    let damaged =
        match surface
            .drm_output
            .render_frame(&mut renderer, &elements, clear, FrameFlags::DEFAULT)
        {
            Ok(result) if result.is_empty => Some(None),
            Ok(result) => {
                Some(Some(state.mirror_source(output).is_none().then(|| {
                    presentation_feedback(state, output, &result.states)
                })))
            }
            Err(err) => {
                warn!("render on {}: {err}", output.name());
                None
            }
        };
    drop(elements);
    if !state.pending_copies.is_empty() && state.mirror_source(output).is_none() {
        crate::screencopy::process(state, renderer.as_mut(), output);
    }
    match damaged? {
        Some(feedback) => match surface.drm_output.queue_frame(feedback) {
            Ok(()) => Some(true),
            Err(err) => {
                warn!("queue_frame on {}: {err}", output.name());
                // The frame never reached the screen, but the damage tracker
                // counts it as drawn: without a reset, static parts such as
                // the wallpaper stay missing until something covers them.
                surface.drm_output.reset_buffers();
                None
            }
        },
        None => Some(false),
    }
}

fn frame_finished(
    state: &mut State,
    node: DrmNode,
    crtc: crtc::Handle,
    metadata: Option<DrmEventMetadata>,
) {
    let Backend::Udev(data) = &mut state.backend else {
        return;
    };
    let Some(surface) = data
        .devices
        .get_mut(&node)
        .and_then(|d| d.surfaces.get_mut(&crtc))
    else {
        return;
    };
    let output = surface.output.clone();
    match surface.drm_output.frame_submitted() {
        Ok(Some(Some(mut feedback))) => {
            let (time, flags) = match metadata.map(|m| m.time) {
                Some(DrmTime::Monotonic(t)) => {
                    (t.into(), Kind::Vsync | Kind::HwClock | Kind::HwCompletion)
                }
                _ => (state.clock.now(), Kind::Vsync),
            };
            let refresh = Refresh::fixed(frame_duration(&output));
            let seq = metadata.map(|m| m.sequence as u64).unwrap_or(0);
            feedback.presented(time, refresh, seq, flags);
        }
        Ok(_) => {}
        Err(err) => warn!("frame_submitted: {err}"),
    }
    vblank(state, &output, true);
}

fn vblank(state: &mut State, output: &Output, real: bool) {
    if !real {
        if let Backend::Udev(data) = &mut state.backend {
            if let Some((device, crtc)) = find_surface(&mut data.devices, output) {
                if let Some(s) = device.surfaces.get_mut(&crtc) {
                    s.vblank_timer = None;
                }
            }
        }
    }
    let queued = match output_state(output).redraw {
        RedrawState::WaitingForVblank { queued } => queued,
        _ => false,
    };
    output_state(output).redraw = RedrawState::Idle;
    // A mirror follows its source, which queues it whenever it redraws.
    if state.mirror_source(output).is_some() {
        if queued {
            queue_redraw(state, output);
        }
        return;
    }
    // Through State so mirrors follow animations frame by frame.
    if queued || state.is_animating(output) || state.interaction.is_some() {
        state.queue_redraw(output);
    }
}

impl State {
    fn render_cache_drop_output(&mut self, _output: &Output) {
        self.relayout_outputs();
    }
}

#[cfg(test)]
mod tests {
    use super::parse_mode;

    #[test]
    fn parses_modes() {
        assert_eq!(parse_mode("2560x1440@165"), Some((2560, 1440, Some(165.0))));
        assert_eq!(parse_mode("1920x1080"), Some((1920, 1080, None)));
        assert_eq!(
            parse_mode("1920x1080@59.94"),
            Some((1920, 1080, Some(59.94)))
        );
        assert_eq!(parse_mode("garbage"), None);
    }
}
