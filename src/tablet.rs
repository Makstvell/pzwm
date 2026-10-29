//! Drawing tablets and pens (tablet-v2): the pen moves the cursor like a
//! mouse and apps that understand tablets (Krita, GIMP, Xournal++) get
//! pressure, tilt and the pen buttons.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Device, DeviceCapability, Event, InputBackend, ProximityState,
        TabletToolButtonEvent, TabletToolEvent, TabletToolProximityEvent, TabletToolTipEvent,
        TabletToolTipState,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
    wayland::{
        seat::WaylandFocus,
        tablet_manager::{TabletDescriptor, TabletSeatTrait},
    },
};

use crate::state::State;

/// Surface under the pen and where it sits, as for pointer focus.
type PenFocus = (WlSurface, Point<f64, Logical>);

impl State {
    pub fn tablet_device_added<D: Device>(&mut self, device: &D) {
        if device.has_capability(DeviceCapability::TabletTool) {
            self.seat
                .tablet_seat()
                .add_tablet::<State>(&self.display_handle, &TabletDescriptor::from(device));
        }
    }

    pub fn tablet_device_removed<D: Device>(&mut self, device: &D) {
        if device.has_capability(DeviceCapability::TabletTool) {
            let seat = self.seat.tablet_seat();
            seat.remove_tablet(&TabletDescriptor::from(device));
            if seat.count_tablets() == 0 {
                seat.clear_tools();
            }
        }
    }

    /// Moves the cursor to where the pen points; tablets map onto the
    /// first output like other absolute devices.
    fn pen_moved<E: AbsolutePositionEvent<B>, B: InputBackend>(&mut self, event: &E) -> bool {
        let Some(output) = self.outputs.first().cloned() else {
            return false;
        };
        let geo = self.output_geometry(&output);
        self.cursor = geo.loc.to_f64() + event.position_transformed(geo.size);
        self.pointer_moved(event.time_msec());
        true
    }

    /// Surface under the pen and the location in its coordinate space.
    fn pen_focus(&self) -> (Point<f64, Logical>, Option<PenFocus>) {
        let under = self.under();
        (self.event_location(&under), self.focus_of(&under))
    }

    pub fn tablet_axis<B: InputBackend>(&mut self, event: B::TabletToolAxisEvent) {
        if !self.pen_moved::<_, B>(&event) {
            return;
        }
        let seat = self.seat.tablet_seat();
        let tablet = seat.get_tablet(&TabletDescriptor::from(&event.device()));
        let tool = seat.get_tool(&event.tool());
        let (Some(tablet), Some(tool)) = (tablet, tool) else {
            return;
        };
        if event.pressure_has_changed() {
            tool.pressure(event.pressure());
        }
        if event.distance_has_changed() {
            tool.distance(event.distance());
        }
        if event.tilt_has_changed() {
            tool.tilt(event.tilt());
        }
        if event.slider_has_changed() {
            tool.slider_position(event.slider_position());
        }
        if event.rotation_has_changed() {
            tool.rotation(event.rotation());
        }
        if event.wheel_has_changed() {
            tool.wheel(event.wheel_delta(), event.wheel_delta_discrete());
        }
        let (location, focus) = self.pen_focus();
        let serial = self.next_serial();
        tool.motion(location, focus, &tablet, serial, event.time_msec());
    }

    pub fn tablet_proximity<B: InputBackend>(&mut self, event: B::TabletToolProximityEvent) {
        let seat = self.seat.tablet_seat();
        let dh = self.display_handle.clone();
        seat.add_tool::<State>(self, &dh, &event.tool());
        if !self.pen_moved::<_, B>(&event) {
            return;
        }
        let tablet = seat.get_tablet(&TabletDescriptor::from(&event.device()));
        let Some(tool) = seat.get_tool(&event.tool()) else {
            return;
        };
        match event.state() {
            ProximityState::In => {
                let (location, focus) = self.pen_focus();
                if let (Some(focus), Some(tablet)) = (focus, tablet) {
                    let serial = self.next_serial();
                    tool.proximity_in(location, focus, &tablet, serial, event.time_msec());
                }
            }
            ProximityState::Out => tool.proximity_out(event.time_msec()),
        }
    }

    pub fn tablet_tip<B: InputBackend>(&mut self, event: B::TabletToolTipEvent) {
        let Some(tool) = self.seat.tablet_seat().get_tool(&event.tool()) else {
            return;
        };
        match event.tip_state() {
            TabletToolTipState::Down => {
                // Touching a window with the pen focuses it, like a click.
                let under = self.under();
                if let Some((surface, _)) = self.focus_of(&under) {
                    let window = self
                        .windows
                        .iter()
                        .find(|m| m.window.wl_surface().as_deref() == Some(&surface))
                        .map(|m| m.window.clone());
                    if window.is_some() && window != self.focused {
                        self.focus_window(window);
                    }
                }
                let serial = self.next_serial();
                tool.tip_down(serial, event.time_msec());
            }
            TabletToolTipState::Up => tool.tip_up(event.time_msec()),
        }
    }

    pub fn tablet_button<B: InputBackend>(&mut self, event: B::TabletToolButtonEvent) {
        if let Some(tool) = self.seat.tablet_seat().get_tool(&event.tool()) {
            let serial = self.next_serial();
            tool.button(
                event.button(),
                event.button_state(),
                serial,
                event.time_msec(),
            );
        }
    }
}
