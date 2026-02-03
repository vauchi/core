// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzz target for recovery claim binary parsing.
//!
//! Tests `RecoveryClaim::from_bytes()` and `RecoveryVoucher::from_bytes()`
//! with arbitrary byte input to find panics in version byte handling,
//! slice conversion, or timestamp validation.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::recovery::{RecoveryClaim, RecoveryVoucher};

fuzz_target!(|data: &[u8]| {
    let _ = RecoveryClaim::from_bytes(data);
    let _ = RecoveryVoucher::from_bytes(data);
});
