// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for ratchet state machine transitions (#180).
//!
//! Feeds arbitrary `RatchetMessage` payloads through an initialized
//! `DoubleRatchetState::decrypt()` to find panics in DH ratchet steps,
//! skipped-key handling, chain derivation, and padding/AEAD validation.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::crypto::ratchet::{DoubleRatchetState, RatchetMessage};
use vauchi_core::crypto::SymmetricKey;

fuzz_target!(|data: &[u8]| {
    // Try to deserialize as a RatchetMessage and decrypt it
    if let Ok(msg) = serde_json::from_slice::<RatchetMessage>(data) {
        let shared_secret = SymmetricKey::generate();
        let dh_pair = vauchi_core::exchange::X3DHKeyPair::generate();
        let mut state = DoubleRatchetState::initialize_responder(&shared_secret, dh_pair);

        // This should never panic, only return errors
        let _ = state.decrypt(&msg);
    }
});
