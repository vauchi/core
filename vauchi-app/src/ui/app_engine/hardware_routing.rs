// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Delivery of hardware commands to the OS's hardware shell (ADR-077). A
//! hardware shell attaches by reporting the device's capabilities; until a
//! platform has one, its display shell keeps receiving every command.

use vauchi_core::exchange_types::DeviceCapabilities;
use vauchi_core::{Command, CommandTarget};

use super::AppEngine;

#[derive(Debug, Default)]
pub(super) struct HardwareRoute {
    attached: bool,
    queue: Vec<Command>,
}

impl AppEngine {
    /// Hardware commands waiting for the hardware shell, in the order Core
    /// emitted them. Empty until a hardware shell has reported in.
    pub fn drain_hardware_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.hardware_route.queue)
    }

    pub(super) fn attach_hardware_shell(&mut self, capabilities: DeviceCapabilities) {
        self.set_device_capabilities(capabilities);
        self.hardware_route.attached = true;
    }

    /// Keeps hardware commands out of a display batch once a hardware shell
    /// is attached, queuing them for [`Self::drain_hardware_commands`].
    pub(super) fn route_to_shells(&mut self, commands: Vec<Command>) -> Vec<Command> {
        if !self.hardware_route.attached {
            return commands;
        }
        let (hardware, display): (Vec<_>, Vec<_>) = commands
            .into_iter()
            .partition(|command| command.target() == CommandTarget::Hardware);
        self.hardware_route.queue.extend(hardware);
        display
    }
}
