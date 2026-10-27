// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
mod bluez;
mod delivery;
mod discovery;
mod host;
mod ports;
mod worker;

#[cfg(target_os = "linux")]
pub use bluez::BlueZDiscovery;
pub use delivery::EventSender;
pub use discovery::BleScanner;
pub use host::{UnsupportedDiscovery, host_discovery};
pub use ports::{Discovery, ExecutorError, HardwareFailure};
