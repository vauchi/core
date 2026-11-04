// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The engine's side of hardware-shell routing (ADR-077); the state lives in
//! [`crate::ui::hardware_route::HardwareRoute`].

use vauchi_core::Command;
use vauchi_core::exchange_types::DeviceCapabilities;

use super::AppEngine;

impl AppEngine {
    /// Hardware commands waiting for the hardware shell, in the order Core
    /// emitted them. Empty until a hardware shell has reported in.
    pub fn drain_hardware_commands(&mut self) -> Vec<Command> {
        self.hardware_route.drain()
    }

    /// The display shell asks; the hardware shell shows no UI (ADR-077).
    pub(super) fn permission_required(&mut self, transport: String) -> Vec<Command> {
        self.pending_commands
            .push_back(Command::RequestPermission { transport });
        self.drain_pending_commands()
    }

    /// The person granted what [`Command::RequestPermission`] asked for: the
    /// ledger re-learns it as the grant affordance does, and the current
    /// screen is composed again so the transport is offered.
    pub(super) fn permission_granted(
        &mut self,
        transport: &str,
    ) -> Result<Vec<Command>, super::presentation_protocol::AppPresentationError> {
        if let Some(requirement) = crate::ui::exchange::mode_selection::parse_requirement(transport)
        {
            self.transport_readiness.note_granted(requirement);
            self.rebuild_exchange_engine();
            let _search_restarted = self
                .engine
                .apply_update(crate::ui::EngineUpdate::TransportPermissionGranted);
        }
        self.initial_commands()
    }

    pub(super) fn attach_hardware_shell(&mut self, capabilities: DeviceCapabilities) {
        self.set_device_capabilities(capabilities.clone());
        self.hardware_route.attach(capabilities);
    }

    /// Splits `commands` the way Core's own batches are split: once a hardware
    /// shell is attached, hardware commands are queued for
    /// [`Self::drain_hardware_commands`]. Bindings route commands they add to a
    /// batch, such as a keychain release, through it.
    pub fn route_to_shells(&mut self, mut commands: Vec<Command>) -> Vec<Command> {
        crate::ui::presentation::title_profiles(&mut commands, || self.t("app.name"));
        let mut display = self.hardware_route.route(commands);
        if self.hardware_route.take_camera_missing() {
            display.push(Command::ShowToast {
                toast: vauchi_core::ToastSpec {
                    message: self.unavailable_message("camera"),
                },
            });
        }
        display
    }
}
