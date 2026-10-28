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

pub fn reconfigure_input(state: &mut State) {
    let input = state.config.input.clone();
    if let Backend::Udev(data) = &mut state.backend {
        data.configure_devices(&input);
    }
}
