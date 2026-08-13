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
//!
//! The broker tests are gated on `network-http` because the
//! `DeviceLinkBroker` trait lives behind that feature.

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

// @scenario: local_device_link :: a ceremony's second leg gets its own slot
#[test]
fn both_legs_of_a_ceremony_can_be_offered() {
    let r = offered_rendezvous();
    // The joiner's response channel: the initiator claims this in
    // `Finalizing` to deliver its response, so it needs a slot of its own.
    r.offer("RESP".to_string(), "b64-joiner-channel".to_string())
        .expect("second leg accepted");

    assert_eq!(
        r.claim("RESP", "b64-initiator-response"),
        Ok("b64-joiner-channel".to_string())
    );
    assert_eq!(
        r.complete(CODE).expect("first leg still intact"),
        None,
        "the legs are independent"
    );
}

// @scenario: local_device_link :: the same code cannot be offered twice
#[test]
fn a_duplicate_code_is_refused() {
    let r = offered_rendezvous();
    assert_eq!(
        r.offer(CODE.to_string(), "b64-other".to_string()),
        Err(RendezvousError::AlreadyOffered)
    );
}

// @scenario: local_device_link :: a ceremony cannot grow past its two legs
#[test]
fn a_third_offer_is_refused_so_this_never_becomes_an_open_relay() {
    let r = offered_rendezvous();
    r.offer("RESP".to_string(), "b64-joiner-channel".to_string())
        .expect("second leg accepted");

    assert_eq!(
        r.offer("THIRD".to_string(), "b64-stranger".to_string()),
        Err(RendezvousError::AlreadyOffered),
        "a ceremony has two legs; a third caller must not get a slot"
    );
}

#[cfg(feature = "network-http")]
mod broker {
    use super::{CODE, OFFERED, RESPONSE};
    use std::sync::Arc;
    use vauchi_app::orchestrator::device_link_relay::DeviceLinkBroker;
    use vauchi_app::orchestrator::local_rendezvous::{
        LocalDeviceLinkBroker, SingleCeremonyRendezvous,
    };
    use vauchi_core::network::NetworkError;

    // @scenario: local_device_link :: a non-relay broker satisfies the same contract
    #[test]
    fn a_local_broker_serves_the_whole_offer_claim_complete_cycle() {
        let rendezvous = Arc::new(SingleCeremonyRendezvous::new());
        let owned = LocalDeviceLinkBroker::new(CODE.to_string(), rendezvous);
        // Held as a trait object on purpose: the machines only ever see
        // `&dyn DeviceLinkBroker`, so that is the shape that must work.
        let broker: &dyn DeviceLinkBroker = &owned;

        let code = broker
            .exchange_offer(OFFERED, Some(300))
            .expect("offer accepted");
        assert_eq!(code, CODE);
        assert_eq!(
            broker.exchange_complete(&code).expect("poll before claim"),
            None
        );
        assert_eq!(
            broker.exchange_claim(&code, RESPONSE).expect("claim"),
            OFFERED
        );
        assert_eq!(
            broker.exchange_complete(&code).expect("poll after claim"),
            Some(RESPONSE.to_string())
        );
    }

    // @scenario: local_device_link :: a refusal reaches the caller as a network error
    #[test]
    fn a_refusal_surfaces_as_a_network_error() {
        let rendezvous = Arc::new(SingleCeremonyRendezvous::new());
        let broker = LocalDeviceLinkBroker::new(CODE.to_string(), rendezvous);
        broker
            .exchange_offer(OFFERED, None)
            .expect("offer accepted");

        let err = broker
            .exchange_claim("WRONG", RESPONSE)
            .expect_err("a wrong code must be refused");

        assert!(
            matches!(err, NetworkError::RelayRejected(_)),
            "a refusal must reach the machine as a rejection, got {err:?}"
        );
    }
}
