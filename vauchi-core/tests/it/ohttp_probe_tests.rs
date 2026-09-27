// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The OHTTP forward-hop probe (problems/2026-05-25-relay-ohttp-forward-hop-502,
//! goal G3): one encapsulated request through relay → gateway, reporting the
//! step that failed. `/health` stayed 200 on both hosts while every
//! encapsulated POST returned 502, so the probe must fail exactly there.

#![cfg(feature = "network-http")]

use crate::common::mock_relay::{CannedResponse, MockRelay};
use vauchi_core::network::ohttp_probe::{OhttpProbeStep, probe_ohttp_forward_hop};

const TIMEOUT_MS: u64 = 2_000;

// @internal
#[test]
fn probe_passes_one_encapsulated_fetch_through_a_working_gateway() {
    let mock = MockRelay::start();
    mock.enable_ohttp_gateway();

    let result = probe_ohttp_forward_hop(&mock.url(), Vec::new(), TIMEOUT_MS);

    assert_eq!(result, Ok(()));
    assert_eq!(mock.ohttp_actions(), vec!["fetch".to_string()]);
    let paths: Vec<String> = mock.received().iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        paths,
        vec!["/v2/ohttp-key".to_string(), "/v2/ohttp".to_string()]
    );
}

// @internal
#[test]
fn probe_reports_the_encapsulated_request_when_the_forward_hop_returns_502() {
    // The 2026-05-25 incident: the key and /health answer, the encapsulated
    // POST does not.
    let mock = MockRelay::start();
    mock.enable_ohttp_gateway();
    mock.queue("ohttp", CannedResponse::status(502));
    mock.queue("ohttp", CannedResponse::status(502));

    let failure = probe_ohttp_forward_hop(&mock.url(), Vec::new(), TIMEOUT_MS)
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
    let mock = MockRelay::start();
    mock.enable_ohttp_gateway();
    mock.queue("ohttp-key", CannedResponse::status(503));

    let failure = probe_ohttp_forward_hop(&mock.url(), Vec::new(), TIMEOUT_MS)
        .expect_err("an unavailable key endpoint must fail the probe");

    assert_eq!(failure.step, OhttpProbeStep::KeyFetch);
    let paths: Vec<String> = mock.received().iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        paths,
        vec!["/v2/ohttp-key".to_string()],
        "no encapsulated request is sent"
    );
}

// @internal
#[test]
fn probe_reports_the_key_config_when_the_key_is_malformed() {
    let mock = MockRelay::start();
    mock.enable_ohttp_gateway();
    mock.queue(
        "ohttp-key",
        CannedResponse {
            status: 200,
            headers: vec![("Content-Type".into(), "application/ohttp-keys".into())],
            body: vec![0xff; 8],
        },
    );

    let failure = probe_ohttp_forward_hop(&mock.url(), Vec::new(), TIMEOUT_MS)
        .expect_err("a malformed key config must fail the probe");

    assert_eq!(failure.step, OhttpProbeStep::KeyConfig);
}
