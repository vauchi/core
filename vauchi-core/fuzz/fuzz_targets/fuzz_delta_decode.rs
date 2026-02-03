// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for versioned delta payload decoding.
//!
//! Tests `VersionedPayload::decode()` with arbitrary byte input to find
//! panics in version tag parsing, CEK wrapped payload decoding, or
//! length field validation in the sync delta protocol.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::sync::delta::VersionedPayload;

fuzz_target!(|data: &[u8]| {
    let _ = VersionedPayload::decode(data);
});
