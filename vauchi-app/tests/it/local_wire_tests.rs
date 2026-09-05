// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire format for local device linking (ADR-070 Phase 1).
//!
//! Everything here is reachable by anyone on the segment, so the bounds
//! matter as much as the happy path (DC-01, CC-14). The limits mirror the
//! relay's, so a local ceremony accepts exactly what a relay one does.

use vauchi_app::orchestrator::local_rendezvous::SingleCeremonyRendezvous;
use vauchi_app::orchestrator::local_wire::{
    LocalRequest, LocalResponse, MAX_CODE_BYTES, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES, WireError,
    decode_request, encode_response, serve,
};

const CODE: &str = "123456";
const OFFERED: &str = "b64-initiator-payload";
const RESPONSE: &str = "b64-joiner-payload";

fn offer_frame(payload: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "action": "exchange_offer",
        "payload": payload,
    }))
    .expect("frame encodes")
}

// ── happy path ───────────────────────────────────────────────────────

// @scenario: local_device_link :: the wire carries a whole ceremony
#[test]
fn a_ceremony_round_trips_over_the_wire() {
    let r = SingleCeremonyRendezvous::new();

    let offer = decode_request(&offer_frame(OFFERED)).expect("offer decodes");
    let code = match serve(&r, offer, CODE) {
        LocalResponse::Offered { code } => code,
        other => panic!("expected Offered, got {other:?}"),
    };

    let poll = LocalRequest::ExchangeComplete { code: code.clone() };
    assert_eq!(
        serve(&r, poll, CODE),
        LocalResponse::Polled { response: None },
        "an unclaimed ceremony polls as pending"
    );

    let claim = LocalRequest::ExchangeClaim {
        code: code.clone(),
        response: RESPONSE.to_string(),
    };
    assert_eq!(
        serve(&r, claim, CODE),
        LocalResponse::Claimed {
            payload: OFFERED.to_string()
        }
    );

    assert_eq!(
        serve(&r, LocalRequest::ExchangeComplete { code }, CODE),
        LocalResponse::Polled {
            response: Some(RESPONSE.to_string())
        }
    );
}

// @scenario: local_device_link :: a response survives the encode round trip
#[test]
fn a_response_encodes_to_json_the_joiner_can_read() {
    let encoded = encode_response(&LocalResponse::Offered {
        code: CODE.to_string(),
    });
    let decoded: LocalResponse = serde_json::from_slice(&encoded).expect("response decodes");
    assert_eq!(
        decoded,
        LocalResponse::Offered {
            code: CODE.to_string()
        }
    );
}

// ── bounds: every input is hostile until proven otherwise ────────────

// @scenario: local_device_link :: an oversized frame is refused before parsing
#[test]
fn an_oversized_frame_is_refused() {
    let frame = vec![b'{'; MAX_FRAME_BYTES + 1];
    assert_eq!(decode_request(&frame), Err(WireError::FrameTooLarge));
}

// @scenario: local_device_link :: an oversized payload is refused
#[test]
fn a_payload_larger_than_the_relay_would_accept_is_refused() {
    let frame = offer_frame(&"A".repeat(MAX_PAYLOAD_BYTES + 1));
    assert_eq!(
        decode_request(&frame),
        Err(WireError::FieldTooLarge),
        "a local ceremony must not accept what a relay one would refuse"
    );
}

// @scenario: local_device_link :: an unbounded code is refused
#[test]
fn an_oversized_code_is_refused() {
    let frame = serde_json::to_vec(&serde_json::json!({
        "action": "exchange_complete",
        "code": "9".repeat(MAX_CODE_BYTES + 1),
    }))
    .expect("frame encodes");
    assert_eq!(decode_request(&frame), Err(WireError::FieldTooLarge));
}

// @scenario: local_device_link :: an unknown action is refused
#[test]
fn an_unknown_action_is_refused() {
    let frame = serde_json::to_vec(&serde_json::json!({
        "action": "exchange_drain_everything",
        "code": CODE,
    }))
    .expect("frame encodes");
    assert_eq!(decode_request(&frame), Err(WireError::Malformed));
}

// @scenario: local_device_link :: malformed bytes are refused
#[test]
fn malformed_bytes_are_refused() {
    assert_eq!(
        decode_request(b"not json at all"),
        Err(WireError::Malformed)
    );
    assert_eq!(decode_request(&[]), Err(WireError::Malformed));
    assert_eq!(
        decode_request(&[0xff, 0xfe, 0x00]),
        Err(WireError::Malformed)
    );
}

// @scenario: local_device_link :: a refusal does not disclose ceremony state
#[test]
fn a_refusal_does_not_disclose_which_state_caused_it() {
    let r = SingleCeremonyRendezvous::new();
    let unknown_code = serve(
        &r,
        LocalRequest::ExchangeComplete {
            code: "999999".into(),
        },
        CODE,
    );

    let offer = decode_request(&offer_frame(OFFERED)).expect("offer decodes");
    let code = match serve(&r, offer, CODE) {
        LocalResponse::Offered { code } => code,
        other => panic!("expected Offered, got {other:?}"),
    };
    let _ = serve(
        &r,
        LocalRequest::ExchangeClaim {
            code: code.clone(),
            response: RESPONSE.into(),
        },
        CODE,
    );
    let already_claimed = serve(
        &r,
        LocalRequest::ExchangeClaim {
            code,
            response: "b64-second-joiner".into(),
        },
        CODE,
    );

    // A prober must not be able to tell "no such ceremony" from "that one
    // is already taken" — the two refusals are indistinguishable.
    assert_eq!(unknown_code, already_claimed);
    assert!(matches!(unknown_code, LocalResponse::Error { .. }));
}
