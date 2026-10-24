// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

#![forbid(unsafe_code)]

mod delivery;
mod discovery;
mod ports;
mod worker;

pub use delivery::EventSender;
pub use discovery::BleScanner;
pub use ports::{Discovery, ExecutorError, HardwareFailure};
