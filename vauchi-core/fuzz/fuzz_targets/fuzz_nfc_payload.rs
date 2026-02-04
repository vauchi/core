// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for NFC Active exchange payload parsing.
//!
//! Tests `ExchangeNfc::from_bytes()` with arbitrary byte input to find
//! panics in magic byte validation, version parsing, or slice handling
//! of 174-byte APDU payloads received during phone-to-phone NFC tap.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::exchange::ExchangeNfc;

fuzz_target!(|data: &[u8]| {
    let _ = ExchangeNfc::from_bytes(data);
});
