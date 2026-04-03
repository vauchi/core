// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `Vauchi::connect()` and `Vauchi::sync()` — OHTTP sync wiring.
//!
//! @feature: sync_privacy
//! @scenario: sync_privacy :: OHTTP key bootstrap on connect
//! @scenario: sync_privacy :: sync gate checks

#![cfg(feature = "network-http")]

use vauchi_core::api::{Vauchi, VauchiSyncOutcome};

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

/// sync() without an identity returns NoIdentity.
#[test]
fn test_sync_no_identity_returns_no_identity() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    let result = vauchi.sync();
    assert!(
        matches!(result, Ok(VauchiSyncOutcome::NoIdentity)),
        "expected NoIdentity, got: {result:?}"
    );
}

/// sync() without calling connect() first returns NotConnected.
#[test]
fn test_sync_not_connected_returns_not_connected() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    // Haven't called connect() — no OHTTP key
    let result = vauchi.sync();
    assert!(
        matches!(result, Ok(VauchiSyncOutcome::NotConnected)),
        "expected NotConnected, got: {result:?}"
    );
}

/// sync() called before the timing deadline returns TooSoon (C1/C2 gate).
#[test]
fn test_sync_too_soon_returns_too_soon() {
    use std::time::{Duration, Instant};

    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    vauchi.set_next_sync_allowed(Instant::now() + Duration::from_secs(3600));
    let result = vauchi.sync().unwrap();
    assert!(
        matches!(result, VauchiSyncOutcome::TooSoon),
        "expected TooSoon, got: {result:?}"
    );
}

/// disconnect() clears the OHTTP key so sync() returns NotConnected.
#[test]
fn test_disconnect_clears_ohttp_state() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    vauchi.disconnect();
    let result = vauchi.sync().unwrap();
    assert!(
        matches!(result, VauchiSyncOutcome::NotConnected),
        "expected NotConnected after disconnect, got: {result:?}"
    );
}
