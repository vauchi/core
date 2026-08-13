// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reproduces the production device-link failure of 2026-08-13.
//!
//! The relay rotates its OHTTP key every 24 h
//! (`RELAY_OHTTP_KEY_ROTATION_HOURS`). A client still holding the previous
//! key id gets `OHTTP decapsulate failed — the key ID was invalid`, which
//! the relay answers with 400 and the OHTTP gateway masks as 502. Observed
//! on the production host:
//!
//! ```text
//! relay:   OHTTP decapsulate failed ... error=ohttp error: the key ID was invalid
//! gateway: upstream gateway error on OHTTP forward error=upstream returned status 400
//! client:  relay offer failed: Connection failed: HTTP 502
//! ```
//!
//! `sync()` already survived this: `api/vauchi/sync_http.rs` evicts the
//! cached key, refetches and retries once. Device linking does not go
//! through `sync()` — it reaches the relay via `DeviceLinkBroker` →
//! `HttpTransport::exchange_offer` → `post_action` — so before the fix it
//! stayed broken after every rotation while sync self-healed
//! (2026-05-25-relay-ohttp-forward-hop-502).

#![cfg(feature = "network-http")]

use crate::common::mock_relay::{CannedResponse, MockRelay};
use vauchi_core::network::OhttpClient;
use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};

/// A syntactically valid OHTTP key config. Its *contents* do not matter
/// here — the relay rejects the key id, and these tests are about what the
/// client does with that rejection.
fn test_ohttp_key_bytes() -> Vec<u8> {
    use ohttp::{KeyConfig, SymmetricSuite, hpke};
    KeyConfig::new(
        0,
        hpke::Kem::X25519Sha256,
        vec![SymmetricSuite::new(
            hpke::Kdf::HkdfSha256,
            hpke::Aead::ChaCha20Poly1305,
        )],
    )
    .expect("KeyConfig::new must succeed")
    .encode()
    .expect("encode must succeed")
}

fn test_ohttp_client() -> OhttpClient {
    OhttpClient::new(test_ohttp_key_bytes())
        .expect("OhttpClient::new must succeed with a valid config")
}

/// The key endpoint's response. Built literally rather than via `ok_json`,
/// which would leave two Content-Type headers — the transport rejects
/// anything but `application/ohttp-keys`.
fn key_response() -> CannedResponse {
    CannedResponse {
        status: 200,
        headers: vec![("Content-Type".into(), "application/ohttp-keys".into())],
        body: test_ohttp_key_bytes(),
    }
}

// @scenario: ohttp_stale_key :: device link recovers from a rotated relay key
#[test]
fn device_link_refetches_the_ohttp_key_after_a_stale_key_rejection() {
    let mock = MockRelay::start();
    // Every OHTTP post is refused the way a rotated key is refused. The key
    // endpoint still answers, as the real relay does — it is only the
    // client's *cached* key that went stale.
    mock.set_default(CannedResponse::status(400));
    mock.queue("ohttp-key", key_response());

    let mut transport = HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), 2_000));
    transport.set_ohttp(test_ohttp_client());

    let result = transport.exchange_offer("b64-offer-payload", Some(300));

    assert!(
        result.is_err(),
        "the retry also hits a refusing relay here, so the call still fails"
    );

    let paths: Vec<String> = mock.received().iter().map(|r| r.path.clone()).collect();

    assert!(
        paths.iter().any(|p| p.contains("ohttp-key")),
        "a stale-key rejection must trigger a key refetch, as the sync path \
         already does — otherwise device linking stays broken until reinstall. \
         Requests seen: {paths:?}"
    );

    // Refetching without retrying would still leave the caller with an error
    // on the very attempt that provoked the refresh.
    assert_eq!(
        paths.iter().filter(|p| p.ends_with("/v2/ohttp")).count(),
        2,
        "the request must be retried once after the key is refreshed. \
         Requests seen: {paths:?}"
    );
}

// @scenario: ohttp_stale_key :: a healthy relay is not asked for a new key
#[test]
fn a_successful_exchange_does_not_refetch_the_key() {
    let mock = MockRelay::start();
    mock.set_default(CannedResponse::ok_json(
        br#"{"status":"ok","code":"123456"}"#.to_vec(),
    ));

    let mut transport = HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), 2_000));
    transport.set_ohttp(test_ohttp_client());

    let _ = transport.exchange_offer("b64-offer-payload", Some(300));

    let paths: Vec<String> = mock.received().iter().map(|r| r.path.clone()).collect();
    assert!(
        !paths.iter().any(|p| p.contains("ohttp-key")),
        "a working key must not be discarded — refetching on every call would \
         double the request count. Requests seen: {paths:?}"
    );
}
