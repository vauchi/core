// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The client side of the relay's cohort actions (ADR-081, proposed;
//! vauchi/private#1281): a session's fixed slots go to the relay as
//! numbered cohort writes, over OHTTP only.
#![cfg(feature = "network-http")]

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;
use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};
use vauchi_core::network::ohttp_client::OhttpClient;
use vauchi_core::notice::session::{FAST, Session};
use vauchi_protocol::cohort::{CohortClass, CohortTier};
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::notice_pair::*;
use crate::common::signed_gateway::{NOW, SignedGateway};

const TIMEOUT_MS: u64 = 2_000;

fn gateway() -> MockRelay {
    let mock = MockRelay::start();
    mock.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    mock
}

fn over_ohttp(mock: &MockRelay) -> HttpTransport {
    let mut http = HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), TIMEOUT_MS));
    http.set_ohttp(OhttpClient::new(mock.ohttp_key_config()).expect("key config"));
    http
}

// @internal
#[test]
fn a_sessions_slots_go_to_the_relay_as_numbered_cohort_writes_over_ohttp() {
    let mut p = pair();
    let mut session = Session::default();
    let written = session.write(&mut [], &mut [&mut p.receiver], &mut [], FAST, START);
    let requests = written.cohort_writes(1, &[3; 32], CohortTier::Fast);
    assert_eq!(requests.len(), FAST.notices + FAST.contents);
    let mock = gateway();
    mock.answer_ohttp_action(
        "cohort_write",
        json!({ "status": "ok", "written": "stored" }),
    );
    let http = over_ohttp(&mock);
    for request in &requests {
        assert!(http.cohort_write(request).expect("answered").is_ok());
    }
    let inner: Vec<serde_json::Value> = mock
        .ohttp_inner_requests()
        .iter()
        .map(|body| serde_json::from_slice(body).expect("JSON"))
        .collect();
    assert_eq!(inner.len(), requests.len());
    let mut notice_slots = Vec::new();
    let mut content_slots = Vec::new();
    for body in &inner {
        assert_eq!(body["action"], "cohort_write");
        assert_eq!(body["cohort"], 1);
        assert_eq!(body["device"], STANDARD.encode([3u8; 32]));
        assert_eq!(body["tier"], "fast");
        let record = STANDARD
            .decode(body["record"].as_str().expect("record"))
            .expect("base64");
        let slot = body["slot"].as_u64().expect("slot");
        match body["class"].as_str() {
            Some("notice") => {
                assert_eq!(record.len(), 96);
                notice_slots.push(slot);
            }
            Some("content") => {
                assert_eq!(record.len(), 576);
                content_slots.push(slot);
            }
            other => panic!("class {other:?}"),
        }
    }
    notice_slots.sort_unstable();
    content_slots.sort_unstable();
    assert_eq!(notice_slots, (0..FAST.notices as u64).collect::<Vec<_>>());
    assert_eq!(content_slots, (0..FAST.contents as u64).collect::<Vec<_>>());
    assert!(requests.iter().any(|r| r.class == CohortClass::Content));
}

// The cohort actions exist only behind the OHTTP gateway; a direct request
// would show the relay the device's address with its slots.
// @internal
#[test]
fn cohort_writes_never_go_direct() {
    let mock = MockRelay::start();
    let http = HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), TIMEOUT_MS));
    let mut p = pair();
    let written = Session::default().write(&mut [], &mut [&mut p.receiver], &mut [], FAST, START);
    let request = written
        .cohort_writes(1, &[3; 32], CohortTier::Fast)
        .remove(0);
    assert!(http.cohort_write(&request).is_err());
    assert!(mock.received().is_empty(), "nothing went to the relay");
}

// @internal
#[test]
fn the_relays_refusal_comes_back_as_its_error() {
    let mock = gateway();
    mock.answer_ohttp_action(
        "cohort_write",
        json!({ "status": "error", "error": "slot_used" }),
    );
    let http = over_ohttp(&mock);
    let mut p = pair();
    let written = Session::default().write(&mut [], &mut [&mut p.receiver], &mut [], FAST, START);
    let request = written
        .cohort_writes(1, &[3; 32], CohortTier::Fast)
        .remove(0);
    let answer = http.cohort_write(&request).expect("answered");
    assert!(!answer.is_ok());
    assert_eq!(answer.error.as_deref(), Some("slot_used"));
}
