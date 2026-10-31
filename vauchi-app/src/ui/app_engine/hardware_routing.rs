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

    pub(super) fn attach_hardware_shell(&mut self, capabilities: DeviceCapabilities) {
        self.set_device_capabilities(capabilities.clone());
        self.hardware_route.attach(capabilities);
    }

    pub(super) fn route_to_shells(&mut self, commands: Vec<Command>) -> Vec<Command> {
        self.hardware_route.route(commands)
    }
}
