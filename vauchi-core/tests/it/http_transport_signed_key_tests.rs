// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The transport's signed gateway key (#288 plan 6.2). The outer relay is on
//! the key-fetch path, so a transport given a relay's anchor never installs
//! a key that does not chain to it — not on bootstrap, and not on the
//! refetch after a stale-key rejection, which used to install whatever
//! `/v2/ohttp-key` returned.

#![cfg(all(feature = "network-http", feature = "testing"))]

use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};
use vauchi_core::network::ohttp_key_trust::HeldOhttpKey;
use vauchi_protocol::escrow::EscrowMessage;
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::{CannedResponse, MockRelay};
use crate::common::signed_gateway::{NOW, SignedGateway, signed_response};

const WINDOW: u64 = 20_367;

fn clock() -> std::sync::Arc<dyn vauchi_core::clock::Clock> {
    vauchi_core::clock::FakeClock::new(std::time::UNIX_EPOCH + std::time::Duration::from_secs(NOW))
        .shared()
}

fn transport(mock: &MockRelay) -> HttpTransport {
    HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), 2_000))
}

fn paths(mock: &MockRelay) -> Vec<String> {
    mock.received()
        .into_iter()
        .map(|request| request.path)
        .collect()
}

fn offer() -> EscrowMessage {
    EscrowMessage::Count {
        gate_hash: "ab".repeat(32),
    }
}

// @internal
#[test]
fn the_signed_record_is_fetched_and_decoded() {
    let mock = MockRelay::start();
    let gateway = SignedGateway::new(0x31);
    mock.queue("ohttp-key-signed", gateway.response(WINDOW));

    let record = transport(&mock).fetch_signed_ohttp_key().expect("record");

    assert_eq!(record, gateway.record(WINDOW));
    assert_eq!(paths(&mock), vec!["/v2/ohttp-key-signed"]);
}

/// DC-04: a response that is not a signed record is refused at the parse
/// boundary, whatever the outer relay put in it.
// @internal
#[test]
fn anything_but_a_signed_record_is_refused() {
    let gateway = SignedGateway::new(0x31);
    let record = gateway.record(WINDOW).encode();
    let cases: Vec<(&str, CannedResponse)> = vec![
        (
            "unsigned key content type",
            CannedResponse {
                status: 200,
                headers: vec![("Content-Type".into(), "application/ohttp-keys".into())],
                body: record.clone(),
            },
        ),
        (
            "truncated record",
            signed_response(record[..record.len() - 1].to_vec()),
        ),
        ("empty body", signed_response(Vec::new())),
        ("oversized body", signed_response(vec![1u8; 64 * 1024])),
        ("no signed key yet", CannedResponse::status(503)),
    ];

    for (case, response) in cases {
        let mock = MockRelay::start();
        mock.queue("ohttp-key-signed", response);

        assert!(
            transport(&mock).fetch_signed_ohttp_key().is_err(),
            "{case} must be refused"
        );
    }
}

fn held(gateway: &SignedGateway, window: u64) -> HeldOhttpKey {
    HeldOhttpKey {
        window,
        key_config: gateway.key_config(window),
    }
}

/// After the gateway refuses the held key, an anchored transport takes the
/// next window's *signed* key and retries with it.
// @internal
#[test]
fn a_stale_key_rejection_refetches_the_signed_record() {
    let mock = MockRelay::start();
    let gateway = SignedGateway::new(0x31);
    let today = window_of(NOW);
    mock.set_default(CannedResponse::status(400));
    mock.queue("ohttp-key-signed", gateway.response(today));
    let mut transport = transport(&mock);
    transport
        .set_signed_ohttp(gateway.anchor(), held(&gateway, today - 1), clock())
        .expect("install the held key");

    let _ = transport.escrow(&offer());

    assert_eq!(
        paths(&mock),
        vec!["/v2/ohttp", "/v2/ohttp-key-signed", "/v2/ohttp"],
        "one refused request, one signed fetch, one retry"
    );
    assert_eq!(
        transport.held_ohttp_key(),
        Some(held(&gateway, today)),
        "the transport now holds today's signed key"
    );
}

/// The hole #288 closes: a refetched key that does not chain to the anchor
/// is never installed, and the unsigned endpoint is never consulted.
// @internal
#[test]
fn a_refetched_record_under_another_anchor_is_never_installed() {
    let mock = MockRelay::start();
    let gateway = SignedGateway::new(0x31);
    let impostor = SignedGateway::new(0x77);
    let today = window_of(NOW);
    mock.set_default(CannedResponse::status(400));
    mock.queue("ohttp-key-signed", impostor.response(today));
    let mut transport = transport(&mock);
    transport
        .set_signed_ohttp(gateway.anchor(), held(&gateway, today - 1), clock())
        .expect("install the held key");

    let result = transport.escrow(&offer());

    assert!(result.is_err());
    assert_eq!(paths(&mock), vec!["/v2/ohttp", "/v2/ohttp-key-signed"]);
    assert_eq!(transport.held_ohttp_key(), Some(held(&gateway, today - 1)));
}

/// Rolling back to an older window is refused even when correctly signed:
/// an outer relay replaying yesterday's record cannot move the client back.
// @internal
#[test]
fn a_refetched_older_record_is_never_installed() {
    let mock = MockRelay::start();
    let gateway = SignedGateway::new(0x31);
    let today = window_of(NOW);
    mock.set_default(CannedResponse::status(400));
    mock.queue("ohttp-key-signed", gateway.response(today - 1));
    let mut transport = transport(&mock);
    transport
        .set_signed_ohttp(gateway.anchor(), held(&gateway, today), clock())
        .expect("install the held key");

    let result = transport.escrow(&offer());

    assert!(result.is_err());
    assert_eq!(transport.held_ohttp_key(), Some(held(&gateway, today)));
}

/// A held key that is not a usable OHTTP key config is refused up front.
// @internal
#[test]
fn installing_a_held_key_that_is_not_a_key_config_fails() {
    let mock = MockRelay::start();
    let gateway = SignedGateway::new(0x31);
    let mut transport = transport(&mock);

    let result = transport.set_signed_ohttp(
        gateway.anchor(),
        HeldOhttpKey {
            window: WINDOW,
            key_config: vec![0x01, 0x02],
        },
        clock(),
    );

    assert!(result.is_err());
    assert_eq!(transport.held_ohttp_key(), None);
}
