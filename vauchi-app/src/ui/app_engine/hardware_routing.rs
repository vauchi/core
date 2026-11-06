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
        let key = transport.trim().to_ascii_lowercase();
        if crate::ui::exchange::mode_selection::parse_requirement(&key).is_none()
            && self.capability_label(&key).is_none()
        {
            return vec![self.permission_settings_alert(&key)];
        }
        if self.pending_permission_requests.insert(key) {
            self.pending_commands
                .push_back(Command::RequestPermission { transport });
        }
        self.drain_pending_commands()
    }

    pub(super) fn permission_settings_alert(&self, transport: &str) -> Command {
        let title = match self.capability_label(transport) {
            Some(name) => crate::i18n::get_string_with_args(
                self.render_context.resolved_locale(),
                "hardware.error_title",
                &[("name", &name)],
            ),
            None => self.t("hardware.error_title_generic"),
        };
        Command::PresentAlert {
            alert: vauchi_core::AlertSpec {
                title,
                message: self.t("hardware.permission_settings_guidance"),
            },
        }
    }

    /// The person granted what [`Command::RequestPermission`] asked for: the
    /// ledger re-learns it as the grant affordance does, and the current
    /// screen is composed again so the transport is offered.
    pub(super) fn permission_granted(
        &mut self,
        transport: &str,
    ) -> Result<Vec<Command>, super::presentation_protocol::AppPresentationError> {
        let key = transport.trim().to_ascii_lowercase();
        self.pending_permission_requests.remove(&key);
        if let Some(requirement) = crate::ui::exchange::mode_selection::parse_requirement(&key) {
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
        if commands.contains(&Command::ResetApplication) {
            self.pending_permission_requests.clear();
        }
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
