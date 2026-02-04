// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for the shared exchange payload parser.
//!
//! Tests `parse_exchange_payload()` with arbitrary byte input and both
//! NFC and BLE magic bytes to find panics in the common 174-byte binary
//! parsing shared by all transport-specific exchange types.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::exchange::exchange_payload::parse_exchange_payload;
use vauchi_core::exchange::ExchangeError;

fuzz_target!(|data: &[u8]| {
    // Test with NFC magic
    let _ = parse_exchange_payload(data, b"VNFC", ExchangeError::InvalidNfcFormat);
    // Test with BLE magic
    let _ = parse_exchange_payload(data, b"VBLE", ExchangeError::InvalidBleFormat);
});
