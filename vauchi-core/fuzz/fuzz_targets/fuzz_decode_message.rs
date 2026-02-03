// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for network message deserialization.
//!
//! Tests `decode_message()` with arbitrary byte input to find
//! panics, infinite loops, or excessive memory allocation in
//! JSON deserialization of `MessageEnvelope`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::network::decode_message;

fuzz_target!(|data: &[u8]| {
    let _ = decode_message(data);
});
