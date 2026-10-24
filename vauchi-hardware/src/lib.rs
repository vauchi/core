// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

#![forbid(unsafe_code)]

mod discovery;

pub use discovery::{BleScanner, Discovery, EventSender, ExecutorError, HardwareFailure};
