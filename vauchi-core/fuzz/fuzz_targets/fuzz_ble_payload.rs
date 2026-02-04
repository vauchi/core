// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for BLE exchange payload parsing.
//!
//! Tests `ExchangeBle::from_bytes()` with arbitrary byte input to find
//! panics in magic byte validation, version parsing, or slice handling
//! of 174-byte GATT payloads received during BLE proximity exchange.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::exchange::ExchangeBle;

fuzz_target!(|data: &[u8]| {
    let _ = ExchangeBle::from_bytes(data);
});
