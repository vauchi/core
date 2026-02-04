// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for BLE advertisement parsing.
//!
//! Tests `BLEAdvertisement::from_bytes()` with arbitrary byte input
//! to find panics in exchange token, public key, or signature slice
//! extraction from 128-byte BLE advertisement payloads.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::exchange::BLEAdvertisement;

fuzz_target!(|data: &[u8]| {
    let _ = BLEAdvertisement::from_bytes(data);
});
