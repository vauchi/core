// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for encrypted exchange message deserialization.
//!
//! Tests `EncryptedExchangeMessage::from_bytes()` with arbitrary byte
//! input to find panics in JSON deserialization of encrypted payloads
//! received from peers during the exchange protocol.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::exchange::EncryptedExchangeMessage;

fuzz_target!(|data: &[u8]| {
    let _ = EncryptedExchangeMessage::from_bytes(data);
});
