//! Nested backend: pzwm runs in a window of the current compositor.
//! Handy for trying the config without leaving your session.

use std::time::Duration;

use smithay::{
    backend::{
        allocator::dmabuf::Dmabuf,
        renderer::{ImportDma, ImportMemWl, damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::{
            EventLoop,
            timer::{TimeoutAction, Timer},
        },
        wayland_server::Display,
    },
    utils::Transform,
    wayland::dmabuf::DmabufState,
};
use tracing::{error, info, warn};

use super::Backend;
use crate::{
    config::Config,
    render::{output_elements, post_render},
    state::State,
};

pub struct WinitData {
    backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    damage_tracker: OutputDamageTracker,
    /// A timer-driven render is pending (works even when the host hides us).
    scheduled: bool,
    /// The host has configured our window; rendering before that blocks EGL.
    ready: bool,
}

impl WinitData {
    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> bool {
        self.backend.renderer().import_dmabuf(dmabuf, None).is_ok()
    }
}

pub fn init(event_loop: &mut EventLoop<'static, State>, config: Config) -> anyhow::Result<State> {
    let display: Display<State> = Display::new()?;
    // Mesa otherwise blocks swaps while the host hides our window, which
    // freezes the whole compositor. Scoped to EGL setup so clients never see it.
    let previous = std::env::var_os("vblank_mode");
    // SAFETY: single-threaded startup.
    unsafe { std::env::set_var("vblank_mode", "0") };
    let init = winit::init::<GlesRenderer>();
    // SAFETY: single-threaded startup.
    unsafe {
        match previous {
            Some(v) => std::env::set_var("vblank_mode", v),
            None => std::env::remove_var("vblank_mode"),
        }
    }
    let (mut backend, winit_loop) = init.map_err(|e| anyhow::anyhow!("winit init: {e}"))?;

    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };
    let output = Output::new(
        "winit".into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "pzwm".into(),
            model: "nested".into(),
        },
    );
    let scale = config.output("winit").and_then(|o| o.scale).unwrap_or(1.0);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        Some(Scale::Fractional(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    let damage_tracker = OutputDamageTracker::from_output(&output);

    let shm_formats = backend.renderer().shm_formats();
    let dmabuf_formats = backend.renderer().dmabuf_formats();

    let data = WinitData {
        backend,
        output: output.clone(),
        damage_tracker,
        scheduled: false,
        ready: false,
    };
    let mut state = State::new(
        display,
        event_loop.handle(),
        event_loop.get_signal(),
        Backend::Winit(Box::new(data)),
        config,
    )?;

    let _global = output.create_global::<State>(&state.display_handle);
    state.outputs.push(output);
    state.shm_state.update_formats(shm_formats);
    let mut dmabuf_state = DmabufState::new();
    let global = dmabuf_state.create_global::<State>(&state.display_handle, dmabuf_formats);
    state.dmabuf_state = Some((dmabuf_state, global));

    event_loop
        .handle()
        .insert_source(winit_loop, |event, _, state| match event {
            WinitEvent::Resized { size, .. } => {
                if let Backend::Winit(data) = &mut state.backend {
                    let mode = Mode {
                        size,
                        refresh: 60_000,
                    };
                    data.output
                        .change_current_state(Some(mode), None, None, None);
                    data.output.set_preferred(mode);
                }
                state.relayout_outputs();
            }
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => {
                if let Backend::Winit(data) = &mut state.backend {
                    data.ready = true;
                }
                queue_redraw(state);
            }
            WinitEvent::CloseRequested => state.loop_signal.stop(),
            WinitEvent::Focus(_) => {}
        })
        .map_err(|e| anyhow::anyhow!("winit source: {e}"))?;

    info!("running nested (winit backend)");
    Ok(state)
}

const FRAME: Duration = Duration::from_millis(16);

pub fn queue_redraw(state: &mut State) {
    let Backend::Winit(data) = &mut state.backend else {
        return;
    };
    if !data.ready {
        data.backend.window().request_redraw();
        return;
    }
    if data.scheduled {
        return;
    }
    data.scheduled = true;
    let _ = state
        .loop_handle
        .insert_source(Timer::from_duration(FRAME), |_, _, state| {
            render(state);
            TimeoutAction::Drop
        });
}

fn render(state: &mut State) {
    let Backend::Winit(mut data) = std::mem::replace(&mut state.backend, Backend::Taken) else {
        return;
    };
    data.scheduled = false;
    let output = data.output.clone();
    let age = data.backend.buffer_age().unwrap_or(0);
    tracing::trace!("winit render: age {age}");

    let damage = match data.backend.bind() {
        Ok((renderer, mut framebuffer)) => {
            tracing::trace!("winit render: bound");
            let (elements, clear) = output_elements(state, renderer, &output);
            tracing::trace!("winit render: {} elements", elements.len());
            match data.damage_tracker.render_output(
                renderer,
                &mut framebuffer,
                age,
                &elements,
                clear,
            ) {
                Ok(result) => result.damage.cloned(),
                Err(err) => {
                    error!("render failed: {err:?}");
                    None
                }
            }
        }
        Err(err) => {
            warn!("bind failed: {err}");
            None
        }
    };
    tracing::trace!(
        "winit render: damage {:?}",
        damage.as_ref().map(|d| d.len())
    );
    if let Some(damage) = damage {
        if let Err(err) = data.backend.submit(Some(&damage)) {
            warn!("submit failed: {err}");
        }
    }
    if !state.pending_copies.is_empty() {
        crate::screencopy::process(state, data.backend.renderer(), &output);
    }
    let animating = state.is_animating(&output);
    state.backend = Backend::Winit(data);
    post_render(state, &output);
    let pending = std::mem::take(&mut crate::state::output_state(&output).redraw_pending);
    if animating || pending {
        queue_redraw(state);
    }
    state.popups.cleanup();
    let _ = state.display_handle.flush_clients();
}
