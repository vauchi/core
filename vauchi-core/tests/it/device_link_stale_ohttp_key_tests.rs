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

//! Since #288 the refetch takes only the gateway's signed record, accepted
//! under the relay's anchor; a transport without an anchor has nothing to
//! refetch and fails closed.

#![cfg(all(feature = "network-http", feature = "testing"))]

use crate::common::mock_relay::{CannedResponse, MockRelay};
use crate::common::signed_gateway::{NOW, SignedGateway};
use vauchi_protocol::escrow::EscrowMessage;
use vauchi_protocol::ohttp_key::window_of;

use vauchi_core::network::OhttpClient;
use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};
use vauchi_core::network::ohttp_key_trust::HeldOhttpKey;

fn clock() -> std::sync::Arc<dyn vauchi_core::clock::Clock> {
    vauchi_core::clock::FakeClock::new(std::time::UNIX_EPOCH + std::time::Duration::from_secs(NOW))
        .shared()
}

/// A transport holding yesterday's signed key, as a client does after the
/// gateway rotated, with today's signed record waiting at the key endpoint.
fn stale_signed_transport(mock: &MockRelay) -> HttpTransport {
    let gateway = SignedGateway::new(0x31);
    let today = window_of(NOW);
    mock.queue("ohttp-key-signed", gateway.response(today));
    let mut transport = HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), 2_000));
    transport
        .set_signed_ohttp(
            gateway.anchor(),
            HeldOhttpKey {
                window: today - 1,
                key_config: gateway.key_config(today - 1),
            },
            clock(),
        )
        .expect("install the held key");
    transport
}

fn test_ohttp_client() -> OhttpClient {
    let config = SignedGateway::new(0x31).key_config(window_of(NOW));
    OhttpClient::new(config).expect("OhttpClient::new must succeed with a valid config")
}

// @scenario: ohttp_stale_key :: device link recovers from a rotated relay key
#[test]
fn device_link_refetches_the_ohttp_key_after_a_stale_key_rejection() {
    let mock = MockRelay::start();
    // Every OHTTP post is refused the way a rotated key is refused. The key
    // endpoint still answers, as the real relay does — it is only the
    // client's held key that went stale.
    mock.set_default(CannedResponse::status(400));
    let transport = stale_signed_transport(&mock);

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

// @scenario: ohttp_stale_key :: link-mode exchange recovers from a rotated relay key
#[test]
fn escrow_refetches_the_ohttp_key_after_a_stale_key_rejection() {
    let mock = MockRelay::start();
    mock.set_default(CannedResponse::status(400));
    let transport = stale_signed_transport(&mock);

    let result = transport.escrow(&EscrowMessage::Count {
        gate_hash: "ab".repeat(32),
    });

    assert!(
        result.is_err(),
        "the retry also hits a refusing relay here, so the call still fails"
    );

    let paths: Vec<String> = mock.received().iter().map(|r| r.path.clone()).collect();

    assert!(
        paths.iter().any(|p| p.contains("ohttp-key")),
        "escrow reaches the relay through its own OHTTP call rather than \
         `post_action`, so it must refresh a rotated key itself — otherwise \
         link-mode exchange deposits nothing and both peers wait forever. \
         Requests seen: {paths:?}"
    );

    assert_eq!(
        paths.iter().filter(|p| p.ends_with("/v2/ohttp")).count(),
        2,
        "the escrow request must be retried once after the key is refreshed. \
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

/// Without an anchor there is no key the transport may take after a
/// rejection: it fails closed instead of asking for the unsigned key the
/// outer relay could substitute (#288 plan 7.5).
// @internal
#[test]
fn an_unanchored_transport_fails_closed_without_fetching_a_key() {
    let mock = MockRelay::start();
    mock.set_default(CannedResponse::status(400));

    let mut transport = HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), 2_000));
    transport.set_ohttp(test_ohttp_client());

    let result = transport.exchange_offer("b64-offer-payload", Some(300));

    assert!(result.is_err(), "the refused request is not retried");
    let paths: Vec<String> = mock.received().iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths, vec!["/v2/ohttp".to_string()], "no key is fetched");
}
