// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for message padding removal.
//!
//! Tests `padding::unpad()` with arbitrary byte input to find panics
//! or incorrect length parsing in the 4-byte BE length prefix format.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::crypto::padding;

fuzz_target!(|data: &[u8]| {
    let _ = padding::unpad(data);
});
