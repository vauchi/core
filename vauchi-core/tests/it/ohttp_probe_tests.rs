// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The OHTTP forward-hop probe (problems/2026-05-25-relay-ohttp-forward-hop-502,
//! goal G3): one encapsulated request through relay → gateway, reporting the
//! step that failed. `/health` stayed 200 on both hosts while every
//! encapsulated POST returned 502, so the probe must fail exactly there.
//! The key it encapsulates to is the gateway's signed record, accepted under
//! the relay's anchor (#288); the unsigned `/v2/ohttp-key` is retired.

#![cfg(feature = "network-http")]

use crate::common::mock_relay::{CannedResponse, MockRelay};
use crate::common::signed_gateway::{NOW, SignedGateway};
use vauchi_core::network::ohttp_probe::{OhttpProbeStep, probe_ohttp_forward_hop};
use vauchi_protocol::ohttp_key::window_of;

const TIMEOUT_MS: u64 = 2_000;

fn gateway_relay(signer: &SignedGateway) -> MockRelay {
    let mock = MockRelay::start();
    mock.enable_ohttp_gateway(signer, window_of(NOW));
    mock
}

fn paths(mock: &MockRelay) -> Vec<String> {
    mock.received().iter().map(|r| r.path.clone()).collect()
}

// @internal
#[test]
fn probe_passes_one_encapsulated_fetch_through_a_working_gateway() {
    let signer = SignedGateway::new(0x41);
    let mock = gateway_relay(&signer);

    let result =
        probe_ohttp_forward_hop(&mock.url(), Vec::new(), &signer.anchor(), NOW, TIMEOUT_MS);

    assert_eq!(result, Ok(()));
    assert_eq!(mock.ohttp_actions(), vec!["fetch".to_string()]);
    assert_eq!(
        paths(&mock),
        vec!["/v2/ohttp-key-signed".to_string(), "/v2/ohttp".to_string()]
    );
}

// @internal
#[test]
fn probe_refuses_a_key_signed_under_another_anchor() {
    let signer = SignedGateway::new(0x41);
    let impostor = SignedGateway::new(0x77);
    let mock = gateway_relay(&impostor);

    let failure =
        probe_ohttp_forward_hop(&mock.url(), Vec::new(), &signer.anchor(), NOW, TIMEOUT_MS)
            .expect_err("a key outside the anchor must fail the probe");

    assert_eq!(failure.step, OhttpProbeStep::KeyConfig);
    assert_eq!(
        paths(&mock),
        vec!["/v2/ohttp-key-signed".to_string()],
        "nothing is encapsulated to an unaccepted key"
    );
}

// @internal
#[test]
fn probe_reports_the_encapsulated_request_when_the_forward_hop_returns_502() {
    // The 2026-05-25 incident: the key and /health answer, the encapsulated
    // POST does not.
    let signer = SignedGateway::new(0x41);
    let mock = gateway_relay(&signer);
    mock.queue("ohttp", CannedResponse::status(502));
    mock.queue("ohttp", CannedResponse::status(502));

    let failure =
        probe_ohttp_forward_hop(&mock.url(), Vec::new(), &signer.anchor(), NOW, TIMEOUT_MS)
            .expect_err("a 502 on the forward hop must fail the probe");

    assert_eq!(failure.step, OhttpProbeStep::EncapsulatedRequest);
    assert!(
        failure.to_string().contains("encapsulated request"),
        "the failure names its step: {failure}"
    );
    assert!(
        mock.ohttp_actions().is_empty(),
        "no request reached the gateway"
    );
}

// @internal
#[test]
fn probe_reports_the_key_fetch_when_the_key_endpoint_is_down() {
    let signer = SignedGateway::new(0x41);
    let mock = gateway_relay(&signer);
    mock.queue("ohttp-key-signed", CannedResponse::status(503));

    let failure =
        probe_ohttp_forward_hop(&mock.url(), Vec::new(), &signer.anchor(), NOW, TIMEOUT_MS)
            .expect_err("an unavailable key endpoint must fail the probe");

    assert_eq!(failure.step, OhttpProbeStep::KeyFetch);
    assert_eq!(
        paths(&mock),
        vec!["/v2/ohttp-key-signed".to_string()],
        "no encapsulated request is sent"
    );
}

// @internal
#[test]
fn probe_reports_the_key_fetch_when_the_record_is_malformed() {
    let signer = SignedGateway::new(0x41);
    let mock = gateway_relay(&signer);
    mock.queue(
        "ohttp-key-signed",
        crate::common::signed_gateway::signed_response(vec![0xff; 8]),
    );

    let failure =
        probe_ohttp_forward_hop(&mock.url(), Vec::new(), &signer.anchor(), NOW, TIMEOUT_MS)
            .expect_err("a malformed record must fail the probe");

    assert_eq!(failure.step, OhttpProbeStep::KeyFetch);
}
