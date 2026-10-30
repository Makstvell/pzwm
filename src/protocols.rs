// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Smaller Wayland protocols that need little more than their global:
//! input methods (fcitx5 and other IMEs: text-input-v3, input-method-v2,
//! virtual-keyboard), xdg-dialog, xdg-foreign (portal dialogs over the
//! right window), alpha-modifier, content-type, xdg-system-bell and
//! tablets (tablet-v2; the events are routed in tablet.rs).

use smithay::{
    desktop::{PopupKind, PopupManager},
    reexports::wayland_server::{DisplayHandle, protocol::wl_surface::WlSurface},
    utils::{Logical, Rectangle},
    wayland::{
        alpha_modifier::AlphaModifierState,
        content_type::ContentTypeState,
        input_method::{InputMethodHandler, InputMethodManagerState, PopupSurface},
        shell::xdg::{
            ToplevelSurface,
            dialog::{XdgDialogHandler, XdgDialogState},
        },
        tablet_manager::TabletManagerState,
        text_input::TextInputManagerState,
        virtual_keyboard::VirtualKeyboardManagerState,
        xdg_foreign::{XdgForeignHandler, XdgForeignState},
        xdg_system_bell::{XdgSystemBellHandler, XdgSystemBellState},
    },
};
use tracing::{debug, warn};

use crate::state::State;

/// Protocol states that are only held so their globals stay alive, plus
/// the ones handlers need to reach.
#[allow(dead_code)]
pub struct Protocols {
    text_input: TextInputManagerState,
    input_method: InputMethodManagerState,
    virtual_keyboard: VirtualKeyboardManagerState,
    dialog: XdgDialogState,
    foreign: XdgForeignState,
    alpha_modifier: AlphaModifierState,
    content_type: ContentTypeState,
    bell: XdgSystemBellState,
    tablet: TabletManagerState,
}

impl Protocols {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            text_input: TextInputManagerState::new::<State>(dh),
            input_method: InputMethodManagerState::new::<State, _>(dh, |_| true),
            virtual_keyboard: VirtualKeyboardManagerState::new::<State, _>(dh, |_| true),
            dialog: XdgDialogState::new::<State>(dh),
            foreign: XdgForeignState::new::<State>(dh),
            alpha_modifier: AlphaModifierState::new::<State>(dh),
            content_type: ContentTypeState::new::<State>(dh),
            bell: XdgSystemBellState::new::<State>(dh),
            tablet: TabletManagerState::new::<State>(dh),
        }
    }
}

impl InputMethodHandler for State {
    fn new_popup(&mut self, surface: PopupSurface) {
        if let Err(err) = self.popups.track_popup(PopupKind::from(surface)) {
            warn!("failed to track input method popup: {err}");
        }
    }

    fn dismiss_popup(&mut self, surface: PopupSurface) {
        if let Some(parent) = surface.get_parent().map(|p| p.surface.clone()) {
            let _ = PopupManager::dismiss_popup(&parent, &PopupKind::from(surface));
        }
    }

    fn popup_repositioned(&mut self, _surface: PopupSurface) {
        self.queue_redraw_all();
    }

    /// The text field's window, in its own coordinates.
    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        self.index_of_surface(parent)
            .map(|i| self.windows[i].window.geometry())
            .unwrap_or_default()
    }
}

impl XdgDialogHandler for State {
    fn modal_changed(&mut self, _toplevel: ToplevelSurface, is_modal: bool) {
        debug!("dialog modal: {is_modal}");
    }
}

impl XdgForeignHandler for State {
    fn xdg_foreign_state(&mut self) -> &mut XdgForeignState {
        &mut self.protocols.foreign
    }
}

impl XdgSystemBellHandler for State {
    fn ring(&mut self, _surface: Option<WlSurface>) {
        debug!("system bell");
    }
}

smithay::delegate_text_input_manager!(State);
smithay::delegate_input_method_manager!(State);
smithay::delegate_virtual_keyboard_manager!(State);
smithay::delegate_xdg_dialog!(State);
smithay::delegate_xdg_foreign!(State);
smithay::delegate_alpha_modifier!(State);
smithay::delegate_content_type!(State);
smithay::delegate_xdg_system_bell!(State);
smithay::delegate_tablet_manager!(State);
