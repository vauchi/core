// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `Vauchi::connect()` — OHTTP key bootstrap.
//!
//! @feature: sync_privacy
//! @scenario: sync_privacy :: OHTTP key bootstrap on connect

#![cfg(feature = "network-http")]

use vauchi_core::api::Vauchi;

/// connect() requires an identity — must return IdentityNotInitialized.
#[test]
fn test_connect_without_identity_returns_error() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    let result = vauchi.connect();
    assert!(result.is_err(), "connect() must fail without an identity");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("identity not initialized"),
        "expected IdentityNotInitialized, got: {err}"
    );
}
