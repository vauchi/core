// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for QR code payload parsing.
//!
//! Tests `ExchangeQR::from_data_string()` with arbitrary string input
//! to find panics in base64 decoding, binary parsing, or signature
//! validation of scanned QR codes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::exchange::ExchangeQR;

fuzz_target!(|data: &[u8]| {
    // ExchangeQR::from_data_string expects a &str (base64-encoded)
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = ExchangeQR::from_data_string(s);
    }
});
