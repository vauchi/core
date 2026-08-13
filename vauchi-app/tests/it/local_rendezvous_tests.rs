// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Host-side rendezvous for local device linking (ADR-070 Phase 1).
//!
//! The relay is a third party that mints a code and holds a payload until
//! the peer claims it. A LAN has none, so the QR-displaying device hosts
//! the rendezvous itself.
//!
//! These pin the rendezvous *semantics*, which nothing else does: the four
//! existing `FakeBroker`s are stubs returning a fixed code and an empty
//! claim, so they pin the machines' call ordering and say nothing about
//! what a broker must actually do.

use vauchi_app::orchestrator::local_rendezvous::{RendezvousError, SingleCeremonyRendezvous};

const CODE: &str = "BROKER123";
const OFFERED: &str = "b64-initiator-payload";
const RESPONSE: &str = "b64-joiner-payload";

fn offered_rendezvous() -> SingleCeremonyRendezvous {
    let r = SingleCeremonyRendezvous::new();
    r.offer(CODE.to_string(), OFFERED.to_string())
        .expect("first offer accepted");
    r
}

// @scenario: local_device_link :: an unclaimed ceremony reports not-yet
#[test]
fn complete_reports_not_yet_before_any_claim() {
    let r = offered_rendezvous();
    assert_eq!(r.complete(CODE), Ok(None));
}

// @scenario: local_device_link :: claiming returns the initiator's payload
#[test]
fn claim_returns_the_offered_payload() {
    let r = offered_rendezvous();
    assert_eq!(r.claim(CODE, RESPONSE), Ok(OFFERED.to_string()));
}

// @scenario: local_device_link :: the initiator collects the peer's response
#[test]
fn complete_returns_the_response_once_claimed() {
    let r = offered_rendezvous();
    let _ = r.claim(CODE, RESPONSE);
    assert_eq!(r.complete(CODE), Ok(Some(RESPONSE.to_string())));
}

// @scenario: local_device_link :: polling twice does not consume the response
#[test]
fn complete_is_repeatable_because_the_machine_polls_per_tick() {
    let r = offered_rendezvous();
    let _ = r.claim(CODE, RESPONSE);
    assert_eq!(r.complete(CODE), Ok(Some(RESPONSE.to_string())));
    assert_eq!(
        r.complete(CODE),
        Ok(Some(RESPONSE.to_string())),
        "a second poll must not consume the response"
    );
}

// @scenario: local_device_link :: a wrong code is refused, not served
#[test]
fn a_wrong_code_is_refused_rather_than_served() {
    let r = offered_rendezvous();
    assert_eq!(r.claim("NOPE", RESPONSE), Err(RendezvousError::UnknownCode));
    assert_eq!(r.complete("NOPE"), Err(RendezvousError::UnknownCode));
}

// @scenario: local_device_link :: exactly one peer wins a contested ceremony
#[test]
fn exactly_one_peer_wins_a_ceremony() {
    let r = offered_rendezvous();
    let _ = r.claim(CODE, RESPONSE);
    assert_eq!(
        r.claim(CODE, "b64-second-joiner"),
        Err(RendezvousError::AlreadyClaimed),
        "a second claim must not overwrite the first peer's response"
    );
    assert_eq!(
        r.complete(CODE),
        Ok(Some(RESPONSE.to_string())),
        "the winner's response survives a losing claim"
    );
}

// @scenario: local_device_link :: one instance serves one ceremony
#[test]
fn one_instance_serves_one_ceremony() {
    let r = offered_rendezvous();
    assert_eq!(
        r.offer(CODE.to_string(), "b64-other".to_string()),
        Err(RendezvousError::AlreadyOffered)
    );
}
