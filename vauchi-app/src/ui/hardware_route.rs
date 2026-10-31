// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Delivery of hardware commands to the OS's hardware shell (ADR-077). A
//! hardware shell attaches by reporting the device's capabilities; until a
//! platform has one, its display shell keeps receiving every command. The
//! report is kept, so a presentation shown before storage opens can hand it
//! to the engine that opens afterwards.

use vauchi_core::exchange_types::DeviceCapabilities;
use vauchi_core::{Command, CommandTarget};

#[derive(Debug, Default)]
pub(crate) struct HardwareRoute {
    report: Option<DeviceCapabilities>,
    queue: Vec<Command>,
}

impl HardwareRoute {
    pub(crate) fn attach(&mut self, capabilities: DeviceCapabilities) {
        self.report = Some(capabilities);
    }

    /// Keeps hardware commands out of a display batch once a hardware shell
    /// is attached, queuing them for [`Self::drain`].
    pub(crate) fn route(&mut self, commands: Vec<Command>) -> Vec<Command> {
        if self.report.is_none() {
            return commands;
        }
        let (hardware, display): (Vec<_>, Vec<_>) = commands
            .into_iter()
            .partition(|command| command.target() == CommandTarget::Hardware);
        self.queue.extend(hardware);
        display
    }

    pub(crate) fn drain(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.queue)
    }
}
