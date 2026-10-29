//! ext-image-copy-capture-v1 with ext-image-capture-source-v1 for outputs:
//! the capture protocol xdg-desktop-portal-wlr prefers for screen sharing
//! (its wlr-screencopy path in 0.8.4 stalls after the first frame). A
//! session tells the client the buffer size and formats once; each frame
//! is then copied by screencopy.rs like a wlr-screencopy copy.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use smithay::{
    output::Output,
    reexports::{
        wayland_protocols::ext::{
            image_capture_source::v1::server::{
                ext_image_capture_source_v1::{self, ExtImageCaptureSourceV1},
                ext_output_image_capture_source_manager_v1::{
                    self, ExtOutputImageCaptureSourceManagerV1,
                },
            },
            image_copy_capture::v1::server::{
                ext_image_copy_capture_cursor_session_v1::{
                    self, ExtImageCopyCaptureCursorSessionV1,
                },
                ext_image_copy_capture_frame_v1::{
                    self, ExtImageCopyCaptureFrameV1, FailureReason,
                },
                ext_image_copy_capture_manager_v1::{self, ExtImageCopyCaptureManagerV1, Options},
                ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
            },
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            backend::ClientId,
            protocol::{wl_buffer::WlBuffer, wl_output::Transform, wl_shm},
        },
    },
    utils::{Physical, Rectangle, Size},
};

use crate::{
    screencopy::{CopyFrame, Failure, PendingCopy},
    state::State,
};

const VERSION: u32 = 1;

pub fn init(dh: &DisplayHandle) {
    dh.create_global::<State, ExtOutputImageCaptureSourceManagerV1, _>(VERSION, ());
    dh.create_global::<State, ExtImageCopyCaptureManagerV1, _>(VERSION, ());
}

/// What a capture source shows: an output, or nothing once it is gone.
pub struct SourceData {
    output: Option<Output>,
}

pub struct SessionData {
    output: Option<Output>,
    cursor: bool,
    /// A frame of this session is waiting to be captured or served.
    frame_live: Arc<AtomicBool>,
}

pub struct FrameData {
    output: Option<Output>,
    cursor: bool,
    buffer: Mutex<Option<WlBuffer>>,
    captured: AtomicBool,
    live: Arc<AtomicBool>,
}

/// A captured frame in the copy queue.
pub struct ExtFrame {
    frame: ExtImageCopyCaptureFrameV1,
    live: Arc<AtomicBool>,
}

impl ExtFrame {
    pub fn ready(&self, size: Size<i32, Physical>, hi: u32, lo: u32, nsec: u32) {
        self.live.store(false, Ordering::Relaxed);
        if !self.frame.is_alive() {
            return;
        }
        // Every copy is a fresh render, so all of it counts as changed.
        self.frame.transform(Transform::Normal);
        self.frame.damage(0, 0, size.w, size.h);
        self.frame.presentation_time(hi, lo, nsec);
        self.frame.ready();
    }

    pub fn failed(&self, why: Failure) {
        self.live.store(false, Ordering::Relaxed);
        if !self.frame.is_alive() {
            return;
        }
        self.frame.failed(match why {
            Failure::Constraints => FailureReason::BufferConstraints,
            Failure::Stopped => FailureReason::Stopped,
            Failure::Unknown => FailureReason::Unknown,
        });
    }
}

fn mode_size(output: &Output) -> Size<i32, Physical> {
    output.current_mode().map(|m| m.size).unwrap_or_default()
}

impl GlobalDispatch<ExtOutputImageCaptureSourceManagerV1, ()> for State {
    fn bind(
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ExtOutputImageCaptureSourceManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ExtOutputImageCaptureSourceManagerV1, ()> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        _manager: &ExtOutputImageCaptureSourceManagerV1,
        request: ext_output_image_capture_source_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        if let ext_output_image_capture_source_manager_v1::Request::CreateSource {
            source,
            output,
        } = request
        {
            let output = Output::from_resource(&output);
            data_init.init(source, SourceData { output });
        }
    }
}

impl Dispatch<ExtImageCaptureSourceV1, SourceData> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        _source: &ExtImageCaptureSourceV1,
        _request: ext_image_capture_source_v1::Request,
        _data: &SourceData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
    }
}

impl GlobalDispatch<ExtImageCopyCaptureManagerV1, ()> for State {
    fn bind(
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ExtImageCopyCaptureManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

/// Starts a session on `output` and tells the client what to allocate;
/// without an output the session is stopped at once.
fn start_session(
    data_init: &mut DataInit<'_, State>,
    session: New<ExtImageCopyCaptureSessionV1>,
    output: Option<Output>,
    cursor: bool,
) {
    let session = data_init.init(
        session,
        SessionData {
            output: output.clone(),
            cursor,
            frame_live: Arc::new(AtomicBool::new(false)),
        },
    );
    let Some(output) = output else {
        session.stopped();
        return;
    };
    let size = mode_size(&output);
    session.buffer_size(size.w as u32, size.h as u32);
    // Opaque first, like for wlr-screencopy: PipeWire clients ask for BGRx.
    session.shm_format(wl_shm::Format::Xrgb8888);
    session.shm_format(wl_shm::Format::Argb8888);
    session.done();
}

impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        _manager: &ExtImageCopyCaptureManagerV1,
        request: ext_image_copy_capture_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            ext_image_copy_capture_manager_v1::Request::CreateSession {
                session,
                source,
                options,
            } => {
                let output = source.data::<SourceData>().and_then(|d| d.output.clone());
                let cursor = options
                    .into_result()
                    .is_ok_and(|o| o.contains(Options::PaintCursors));
                start_session(data_init, session, output, cursor);
            }
            ext_image_copy_capture_manager_v1::Request::CreatePointerCursorSession {
                session,
                ..
            } => {
                // The cursor is painted into the output capture instead.
                data_init.init(session, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureCursorSessionV1, ()> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        _cursor_session: &ExtImageCopyCaptureCursorSessionV1,
        request: ext_image_copy_capture_cursor_session_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        if let ext_image_copy_capture_cursor_session_v1::Request::GetCaptureSession { session } =
            request
        {
            start_session(data_init, session, None, false);
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, SessionData> for State {
    fn request(
        _state: &mut State,
        _client: &Client,
        session: &ExtImageCopyCaptureSessionV1,
        request: ext_image_copy_capture_session_v1::Request,
        data: &SessionData,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        if let ext_image_copy_capture_session_v1::Request::CreateFrame { frame } = request {
            if data.frame_live.swap(true, Ordering::Relaxed) {
                session.post_error(
                    ext_image_copy_capture_session_v1::Error::DuplicateFrame,
                    "the session already has a frame",
                );
                return;
            }
            data_init.init(
                frame,
                FrameData {
                    output: data.output.clone(),
                    cursor: data.cursor,
                    buffer: Mutex::new(None),
                    captured: AtomicBool::new(false),
                    live: data.frame_live.clone(),
                },
            );
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, FrameData> for State {
    fn request(
        state: &mut State,
        _client: &Client,
        frame: &ExtImageCopyCaptureFrameV1,
        request: ext_image_copy_capture_frame_v1::Request,
        data: &FrameData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            ext_image_copy_capture_frame_v1::Request::AttachBuffer { buffer } => {
                if let Ok(mut slot) = data.buffer.lock() {
                    *slot = Some(buffer);
                }
            }
            ext_image_copy_capture_frame_v1::Request::DamageBuffer {
                x,
                y,
                width,
                height,
            } => {
                // Every frame is copied whole, so the hint itself is not needed.
                if x < 0 || y < 0 || width <= 0 || height <= 0 {
                    frame.post_error(
                        ext_image_copy_capture_frame_v1::Error::InvalidBufferDamage,
                        "damage must be inside the buffer",
                    );
                }
            }
            ext_image_copy_capture_frame_v1::Request::Capture => {
                if data.captured.swap(true, Ordering::Relaxed) {
                    frame.post_error(
                        ext_image_copy_capture_frame_v1::Error::AlreadyCaptured,
                        "capture was already sent",
                    );
                    return;
                }
                let Some(buffer) = data.buffer.lock().ok().and_then(|b| b.clone()) else {
                    frame.post_error(
                        ext_image_copy_capture_frame_v1::Error::NoBuffer,
                        "attach a buffer before capture",
                    );
                    return;
                };
                let ext = ExtFrame {
                    frame: frame.clone(),
                    live: data.live.clone(),
                };
                let Some(output) = data.output.clone().filter(|o| state.outputs.contains(o)) else {
                    ext.failed(Failure::Stopped);
                    return;
                };
                state.pending_copies.push(PendingCopy {
                    frame: CopyFrame::Ext(ext),
                    buffer,
                    region: Rectangle::from_size(mode_size(&output)),
                    output: output.clone(),
                    cursor: data.cursor,
                });
                state.queue_redraw(&output);
            }
            _ => {}
        }
    }

    fn destroyed(
        _state: &mut State,
        _client: ClientId,
        _frame: &ExtImageCopyCaptureFrameV1,
        data: &FrameData,
    ) {
        // A frame dropped before it was served frees the session for the next.
        data.live.store(false, Ordering::Relaxed);
    }
}
