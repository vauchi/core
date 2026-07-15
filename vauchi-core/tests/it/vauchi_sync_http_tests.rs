// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `Vauchi::connect()` and `Vauchi::sync()` — OHTTP sync wiring.
//!
//! @feature: sync_privacy
//! @scenario: sync_privacy :: OHTTP key bootstrap on connect
//! @scenario: sync_privacy :: sync gate checks

#![cfg(feature = "network-http")]

use vauchi_core::api::{Vauchi, VauchiConfig, VauchiSyncOutcome};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;

use crate::common::mock_relay::MockRelay;

/// Build a valid OHTTP key using the ohttp crate's server-side KeyConfig.
///
/// Uses the same cipher suite as the unit tests in `ohttp_client.rs`.
/// Only available because dev-dependencies include the `server` feature of `ohttp`.
#[cfg(feature = "testing")]
fn make_test_ohttp_client() -> OhttpClient {
    use ohttp::{KeyConfig, SymmetricSuite, hpke};
    let config = KeyConfig::new(
        0,
        hpke::Kem::X25519Sha256,
        vec![SymmetricSuite::new(
            hpke::Kdf::HkdfSha256,
            hpke::Aead::ChaCha20Poly1305,
        )],
    )
    .expect("KeyConfig::new must succeed");
    let encoded = config.encode().expect("encode must succeed");
    OhttpClient::new(encoded).expect("OhttpClient::new must succeed with valid config")
}

fn vauchi_with_split_relays(
    application_relay: &MockRelay,
    outer_relay: &MockRelay,
) -> (Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application_relay.url())
        .with_ohttp_relay_url(outer_relay.url());
    (Vauchi::new(config).expect("create Vauchi"), dir)
}

// @scenario: sync_privacy:OHTTP sync gate checks
/// connect() requires an identity — must return IdentityNotInitialized.
// @internal
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

// @scenario: sync_privacy:OHTTP sync gate checks
/// sync() without an identity returns NoIdentity.
// @internal
#[test]
fn test_sync_no_identity_returns_no_identity() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    let result = vauchi.sync();
    assert!(
        matches!(result, Ok(VauchiSyncOutcome::NoIdentity)),
        "expected NoIdentity, got: {result:?}"
    );
}

// @scenario: sync_privacy:OHTTP sync gate checks
/// sync() without calling connect() first returns NotConnected.
// @internal
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

// @scenario: sync_privacy:OHTTP sync gate checks
/// sync() called before the timing deadline returns TooSoon (C1/C2 gate).
///
/// Requires a valid OHTTP key to be injected so the NotConnected gate is bypassed
/// and the timing check is actually reached.
// @internal
#[test]
#[cfg(feature = "testing")]
fn test_sync_too_soon_returns_too_soon() {
    use std::time::{Duration, Instant};

    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    // Inject a valid OHTTP key so NotConnected gate passes.
    vauchi.set_ohttp_key_for_testing(make_test_ohttp_client());
    vauchi.set_next_sync_allowed(Instant::now() + Duration::from_secs(3600));
    let result = vauchi.sync().unwrap();
    assert!(
        matches!(result, VauchiSyncOutcome::TooSoon),
        "expected TooSoon, got: {result:?}"
    );
}

// @scenario: sync_privacy:OHTTP key bootstrap on connect
/// disconnect() clears the OHTTP key so sync() returns NotConnected.
// @internal
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

// @scenario: sync_privacy:OHTTP sync gate checks
/// set_post_exchange_delay() sets a timing deadline that causes sync() to return TooSoon.
///
/// Requires a valid OHTTP key to be injected so the NotConnected gate is bypassed.
// @internal
#[test]
#[cfg(feature = "testing")]
fn test_set_post_exchange_delay_blocks_sync() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();

    // Inject a valid OHTTP key so the NotConnected gate passes.
    vauchi.set_ohttp_key_for_testing(make_test_ohttp_client());

    // set_post_exchange_delay() records a future deadline (C1 post-exchange jitter).
    vauchi.set_post_exchange_delay();

    // With a valid OHTTP key and a future deadline, TooSoon must be returned.
    let result = vauchi.sync().unwrap();
    assert!(
        matches!(result, VauchiSyncOutcome::TooSoon),
        "expected TooSoon after set_post_exchange_delay, got: {result:?}"
    );
}

// @scenario: sync_privacy:OHTTP sync gate checks
/// The C1 post-exchange timing gate is driven by the injected
/// `MonotonicClock`, not ambient `Instant::now()`. Advancing a
/// `FakeMonotonicClock` past the recorded deadline must release the
/// gate — proving `set_post_exchange_delay` / `sync` route through the
/// seam (Phase 1 / Task 1.1b). Before the migration this test fails:
/// advancing the fake clock has no effect because the timing math reads
/// the real OS clock, so `sync` stays `TooSoon` forever.
// @internal
#[test]
#[cfg(feature = "testing")]
fn test_post_exchange_delay_gate_driven_by_monotonic_clock() {
    use std::sync::Arc;
    use std::time::Duration;
    use vauchi_core::monotonic::FakeMonotonicClock;

    let fake = Arc::new(FakeMonotonicClock::new());
    let mut vauchi = Vauchi::in_memory().unwrap().with_monotonic(fake.clone());
    vauchi.create_identity("Test User").unwrap();
    vauchi.set_ohttp_key_for_testing(make_test_ohttp_client());

    // Records next_sync_allowed = monotonic.now() + jittered C1 delay
    // (bounded by post_exchange_delay_max_ms, well under one hour).
    vauchi.set_post_exchange_delay();
    assert!(
        matches!(vauchi.sync().unwrap(), VauchiSyncOutcome::TooSoon),
        "fresh post-exchange delay must gate sync as TooSoon"
    );

    // Advance the injected clock well past any possible C1 deadline.
    fake.advance(Duration::from_secs(3600));

    // Gate released: sync proceeds past the timing check (then fails on
    // the offline relay). The point is it is no longer TooSoon — the
    // injected clock, not wall-clock, controls the gate.
    let after = vauchi.sync();
    assert!(
        !matches!(after, Ok(VauchiSyncOutcome::TooSoon)),
        "advancing the injected monotonic clock must release the C1 gate, got: {after:?}"
    );
}

// @scenario: sync_privacy:OHTTP key bootstrap on connect
/// After connect() fails (no real relay), disconnect() still leaves sync() returning NotConnected.
// @internal
#[test]
fn test_sync_outcome_not_connected_after_disconnect() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();

    // connect() will fail — no relay is running. That is expected.
    let _ = vauchi.connect();

    vauchi.disconnect();

    let result = vauchi.sync().unwrap();
    assert!(
        matches!(result, VauchiSyncOutcome::NotConnected),
        "expected NotConnected after disconnect, got: {result:?}"
    );
}

// @scenario: sync_privacy:OHTTP key bootstrap on connect
/// OhttpConfig::default() has the correct production defaults.
// @internal
#[test]
fn test_ohttp_config_defaults() {
    use vauchi_core::api::OhttpConfig;

    let cfg = OhttpConfig::default();
    assert_eq!(
        cfg.key_ttl_secs, 43200,
        "default key_ttl_secs must be 43200 (12 h)"
    );
    assert!(
        !cfg.allow_direct,
        "allow_direct must be false in production defaults"
    );
}

// @scenario: sync_privacy:Application actions fail closed before OHTTP bootstrap
/// An application-action transport built before OHTTP bootstrap must not reach
/// the application relay over its direct JSON endpoint.
// @internal
#[test]
fn test_relay_transport_without_ohttp_key_sends_no_application_request() {
    let application_relay = MockRelay::start();
    let outer_relay = MockRelay::start();
    let (vauchi, _dir) = vauchi_with_split_relays(&application_relay, &outer_relay);
    let transport = vauchi.build_relay_transport(1_000);

    let _ = transport.exchange_offer("opaque-offer", Some(60));

    let requests = application_relay.received();
    assert!(
        requests.is_empty(),
        "missing OHTTP key must fail closed before contacting the application relay; got paths: {:?}",
        requests
            .iter()
            .map(|request| &request.path)
            .collect::<Vec<_>>()
    );
}

// @scenario: sync_privacy:Application actions use the distinct outer OHTTP hop
/// A cached OHTTP key must not cause the client to send `/v2/ohttp` directly to
/// the application relay. The distinct outer relay is the only valid peer.
// @internal
#[test]
#[cfg(feature = "testing")]
fn test_relay_transport_with_ohttp_key_sends_no_direct_ohttp_request() {
    let application_relay = MockRelay::start();
    let outer_relay = MockRelay::start();
    let (mut vauchi, _dir) = vauchi_with_split_relays(&application_relay, &outer_relay);
    vauchi.set_ohttp_key_for_testing(make_test_ohttp_client());
    let transport = vauchi.build_relay_transport(1_000);

    let _ = transport.exchange_offer("opaque-offer", Some(60));

    let requests = application_relay.received();
    assert!(
        requests.is_empty(),
        "cached OHTTP key must not bypass the outer relay; got paths: {:?}",
        requests
            .iter()
            .map(|request| &request.path)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        outer_relay
            .received()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/v2/ohttp"],
        "cached OHTTP request must use the distinct outer relay"
    );
}

// @scenario: sync_privacy:OHTTP key cache persistence
/// Storage roundtrip: save an OHTTP key via storage, load it back, verify bytes match.
// @internal
#[test]
fn test_ohttp_cache_roundtrip_via_storage() {
    let vauchi = Vauchi::in_memory().unwrap();
    let storage = vauchi.storage();

    let relay_url = "https://relay.example.test";
    let key_bytes: Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];

    storage
        .ohttp_cache()
        .save_ohttp_key(relay_url, &key_bytes)
        .unwrap();

    let loaded = storage.ohttp_cache().load_ohttp_key(relay_url).unwrap();
    assert!(loaded.is_some(), "loaded key must be present after save");

    let (loaded_bytes, fetched_at) = loaded.unwrap();
    assert_eq!(
        loaded_bytes, key_bytes,
        "loaded bytes must match saved bytes"
    );
    assert!(fetched_at > 0, "fetched_at timestamp must be non-zero");
}

// @scenario: sync_privacy:OHTTP sync gate checks
/// Sync gate ordering: NoIdentity is checked before NotConnected,
/// and NotConnected is checked before TooSoon.
// @internal
#[test]
#[cfg(feature = "testing")]
fn test_sync_gate_ordering() {
    use std::time::{Duration, Instant};

    // Gate 1: NoIdentity — no identity, no OHTTP key, TooSoon deadline set.
    // Must return NoIdentity, not NotConnected or TooSoon.
    {
        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.set_next_sync_allowed(Instant::now() + Duration::from_secs(3600));
        let result = vauchi.sync().unwrap();
        assert!(
            matches!(result, VauchiSyncOutcome::NoIdentity),
            "identity gate must fire before connection gate, got: {result:?}"
        );
    }

    // Gate 2: NotConnected — identity present, no OHTTP key, TooSoon deadline set.
    // Must return NotConnected, not TooSoon.
    {
        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Test User").unwrap();
        vauchi.set_next_sync_allowed(Instant::now() + Duration::from_secs(3600));
        // No connect() — ohttp_key is None
        let result = vauchi.sync().unwrap();
        assert!(
            matches!(result, VauchiSyncOutcome::NotConnected),
            "connection gate must fire before timing gate, got: {result:?}"
        );
    }

    // Gate 3: TooSoon — identity present, valid OHTTP key injected, future deadline set.
    // Must return TooSoon (both identity and connection gates pass).
    {
        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Test User").unwrap();
        vauchi.set_ohttp_key_for_testing(make_test_ohttp_client());
        vauchi.set_next_sync_allowed(Instant::now() + Duration::from_secs(3600));
        let result = vauchi.sync().unwrap();
        assert!(
            matches!(result, VauchiSyncOutcome::TooSoon),
            "timing gate must fire after identity+connection gates pass, got: {result:?}"
        );
    }
}
