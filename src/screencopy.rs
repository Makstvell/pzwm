//! wlr-screencopy (shm buffers): lets `grim`, `slurp`-based screenshots and
//! screen recorders capture pzwm outputs.

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, Offscreen,
            damage::OutputDamageTracker,
            gles::{GlesRenderer, GlesTexture},
        },
    },
    output::Output,
    reexports::{
        wayland_protocols_wlr::screencopy::v1::server::{
            zwlr_screencopy_frame_v1::{self, Flags, ZwlrScreencopyFrameV1},
            zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            protocol::{wl_buffer::WlBuffer, wl_shm},
        },
    },
    utils::{Buffer as BufferCoords, Physical, Rectangle, Size, Transform},
    wayland::shm::with_buffer_contents_mut,
};
use std::sync::atomic::{AtomicBool, Ordering};

use tracing::warn;

use crate::state::State;

const VERSION: u32 = 3;

pub struct FrameData {
    output: Output,
    /// Captured area in output physical pixels.
    region: Rectangle<i32, Physical>,
    cursor: bool,
    used: AtomicBool,
}

pub struct PendingCopy {
    frame: ZwlrScreencopyFrameV1,
    buffer: WlBuffer,
    output: Output,
    region: Rectangle<i32, Physical>,
    cursor: bool,
}

pub fn init(dh: &DisplayHandle) {
    dh.create_global::<State, ZwlrScreencopyManagerV1, _>(VERSION, ());
}

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn bind(
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        _manager: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let (frame, cursor, wl_output, region) = match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput {
                frame,
                overlay_cursor,
                output,
            } => (frame, overlay_cursor != 0, output, None),
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion {
                frame,
                overlay_cursor,
                output,
                x,
                y,
                width,
                height,
            } => (
                frame,
                overlay_cursor != 0,
                output,
                Some((x, y, width, height)),
            ),
            _ => return,
        };
        let Some(output) = Output::from_resource(&wl_output) else {
            let frame = data_init.init(frame, None);
            frame.failed();
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let mode = output.current_mode().map(|m| m.size).unwrap_or_default();
        let full = Rectangle::<i32, Physical>::from_size(mode);
        let region = match region {
            Some((x, y, w, h)) => {
                let logical = Rectangle::<f64, smithay::utils::Logical>::new(
                    (x as f64, y as f64).into(),
                    (w as f64, h as f64).into(),
                );
                logical
                    .to_physical(scale)
                    .to_i32_round()
                    .intersection(full)
                    .unwrap_or_default()
            }
            None => full,
        };
        let frame = data_init.init(
            frame,
            Some(FrameData {
                output,
                region,
                cursor,
                used: AtomicBool::new(false),
            }),
        );
        if region.is_empty() {
            frame.failed();
            return;
        }
        frame.buffer(
            wl_shm::Format::Argb8888,
            region.size.w as u32,
            region.size.h as u32,
            region.size.w as u32 * 4,
        );
        if frame.version() >= 3 {
            frame.buffer_done();
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, Option<FrameData>> for State {
    fn request(
        state: &mut State,
        _client: &Client,
        frame: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        data: &Option<FrameData>,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        let buffer = match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer }
            | zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => buffer,
            _ => return,
        };
        let Some(data) = data else { return };
        if data.used.swap(true, Ordering::Relaxed) {
            frame.post_error(
                zwlr_screencopy_frame_v1::Error::AlreadyUsed,
                "frame already copied",
            );
            return;
        }
        state.pending_copies.push(PendingCopy {
            frame: frame.clone(),
            buffer,
            output: data.output.clone(),
            region: data.region,
            cursor: data.cursor,
        });
        let output = data.output.clone();
        state.queue_redraw(&output);
    }
}

/// Fails copies still waiting on an output that just went away, so capture
/// clients (the portal, grim) give up instead of waiting forever.
pub fn fail_output(state: &mut State, output: &Output) {
    state.pending_copies.retain(|p| {
        if &p.output == output {
            p.frame.failed();
            false
        } else {
            true
        }
    });
}

/// Serves pending copies for `output` with a fresh render into a texture.
pub fn process(state: &mut State, renderer: &mut GlesRenderer, output: &Output) {
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut state.pending_copies)
        .into_iter()
        .partition(|p| &p.output == output);
    state.pending_copies = rest;
    for copy in mine {
        let result = copy_one(state, renderer, &copy);
        match result {
            Ok(()) => {
                copy.frame.flags(Flags::empty());
                let t: std::time::Duration = state.clock.now().into();
                let secs = t.as_secs();
                copy.frame
                    .ready((secs >> 32) as u32, secs as u32, t.subsec_nanos());
            }
            Err(err) => {
                warn!("screencopy failed: {err}");
                copy.frame.failed();
            }
        }
    }
}

fn copy_one(
    state: &mut State,
    renderer: &mut GlesRenderer,
    copy: &PendingCopy,
) -> anyhow::Result<()> {
    let output = &copy.output;
    let mode = output.current_mode().map(|m| m.size).unwrap_or_default();
    let scale = output.current_scale().fractional_scale();

    let hide_cursor = !copy.cursor;
    let saved = state.render_cursor;
    state.render_cursor = !hide_cursor;
    let (elements, clear) = crate::render::output_elements(state, renderer, output);
    state.render_cursor = saved;

    let buffer_size = Size::<i32, BufferCoords>::from((mode.w, mode.h));
    let mut texture: GlesTexture = renderer.create_buffer(Fourcc::Abgr8888, buffer_size)?;
    let mut target = renderer.bind(&mut texture)?;
    let mut tracker = OutputDamageTracker::new(mode, scale, Transform::Normal);
    tracker
        .render_output(renderer, &mut target, 0, &elements, clear)
        .map_err(|e| anyhow::anyhow!("render: {e:?}"))?;
    let r = copy.region;
    let full = Rectangle::<i32, BufferCoords>::from_size(buffer_size);
    let mapping = renderer.copy_framebuffer(&target, full, Fourcc::Abgr8888)?;
    drop(target);
    let pixels = renderer.map_texture(&mapping)?.to_vec();

    let written = with_buffer_contents_mut(&copy.buffer, |ptr, len, info| {
        let (w, h) = (r.size.w as usize, r.size.h as usize);
        let stride = info.stride as usize;
        let offset = info.offset as usize;
        if info.width as usize != w
            || info.height as usize != h
            || !matches!(
                info.format,
                wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888
            )
            || offset + stride * h > len
        {
            return false;
        }
        // SAFETY: bounds checked above; the pool mapping is valid for `len` bytes.
        let dst = unsafe { std::slice::from_raw_parts_mut(ptr.add(offset), stride * h) };
        let full_w = mode.w as usize;
        let full_h = mode.h as usize;
        // On DRM outputs the readback is already top-down. The nested
        // (winit) output is y-up (Flipped180), so its rows come out reversed.
        let bottom_up = output.current_transform() == Transform::Flipped180;
        for y in 0..h {
            let row = r.loc.y as usize + y;
            let src_y = if bottom_up { full_h - 1 - row } else { row };
            let src_start = (src_y * full_w + r.loc.x as usize) * 4;
            let src = &pixels[src_start..src_start + w * 4];
            let dst_row = &mut dst[y * stride..y * stride + w * 4];
            for (d, s) in dst_row.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                // RGBA (GL) -> BGRA (wl_shm ARGB8888, little endian)
                d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
            }
        }
        true
    })
    .map_err(|e| anyhow::anyhow!("buffer: {e:?}"))?;
    if written {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "client buffer has the wrong size or format"
        ))
    }
}
