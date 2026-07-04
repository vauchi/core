// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end coercion-safety alert flow: send (disguised card_delta) →
//! receive → surface as an Emergency/Duress event.
//!
//! Regression suite for 2026-07-04-coercion-safety-alerts-never-received: the
//! send path existed but the receive path parsed every payload as a CardDelta
//! and dropped alerts, so no alert was ever surfaced.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::delta::VersionedPayload;
use vauchi_core::sync::safety_alert::{AlertKind, SafetyAlertPayload};

/// Two mutual contacts with ratchets stored: Bob is the initiator (can encrypt
/// / send), Alice the responder (can decrypt / receive). Mirrors the harness in
/// `sync_card_update_tests` — kept local so the alert suite is self-contained.
///
/// Returns `(alice_wb, bob_wb, bob_contact_id_at_alice, alice_contact_id_at_bob)`.
fn setup_two_party() -> (vauchi_core::Vauchi, vauchi_core::Vauchi, String, String) {
    let alice_wb = create_vauchi_with_identity("Alice");
    let bob_wb = create_vauchi_with_identity("Bob");

    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let shared_secret = SymmetricKey::generate();

    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone(), 0);
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        shared_secret.clone(),
        0,
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    let alice_dh = X3DHKeyPair::generate();
    let bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *alice_dh.public_key()).unwrap();
    let alice_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, alice_dh);

    alice_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_contact_id, &alice_ratchet, false)
        .unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, true)
        .unwrap();

    (alice_wb, bob_wb, bob_contact_id, alice_contact_id)
}

/// Build a safety-alert blob Bob sends to Alice: a signed `SafetyAlertPayload`
/// (`VersionedPayload` 0x04) encrypted with Bob's ratchet — the same envelope
/// as a card update on the wire (ADR-032).
fn create_alert_blob(
    bob_wb: &vauchi_core::Vauchi,
    alice_signing_pk: &[u8; 32],
    alice_contact_id: &str,
    kind: AlertKind,
    nonce: [u8; 32],
) -> Vec<u8> {
    let bob_identity = bob_wb.identity().unwrap();
    let alert = SafetyAlertPayload::new(
        kind,
        "help me".into(),
        1_720_000_000,
        None,
        nonce,
        bob_identity,
        alice_signing_pk,
    )
    .unwrap();
    let payload = VersionedPayload::encode_alert(&alert);

    let (mut bob_ratchet, is_init) = bob_wb
        .storage()
        .ratchets()
        .load_ratchet_state(alice_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = bob_ratchet.encrypt(&payload).unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(alice_contact_id, &bob_ratchet, is_init)
        .unwrap();
    serde_json::to_vec(&ratchet_msg).unwrap()
}

// @internal
#[test]
fn received_duress_alert_is_verified_and_surfaced() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    let blob = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Duress,
        [9u8; 32],
    );

    let outcome = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &blob,
    )
    .expect("a signed alert must be received, not dropped");
    match outcome {
        ReceiveOutcome::Alert(a) => {
            assert_eq!(a.kind, AlertKind::Duress);
            assert_eq!(a.message, "help me");
            assert_eq!(a.timestamp, 1_720_000_000);
        }
        other => panic!("expected an Alert outcome, got {other:?}"),
    }
}

// @internal
#[test]
fn replayed_alert_is_rejected() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    // Two blobs carrying the SAME alert nonce (a re-sent / captured alert): the
    // first surfaces, the second must be rejected by the replay check.
    let blob1 = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Emergency,
        [3u8; 32],
    );
    let blob2 = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Emergency,
        [3u8; 32],
    );

    let alice_identity = alice_wb.identity().unwrap();
    let first =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob1);
    assert!(
        matches!(first, Ok(ReceiveOutcome::Alert(_))),
        "the first alert must surface, got {first:?}"
    );

    let second =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob2);
    assert!(
        matches!(second, Err(CardUpdateError::ReplayDetected)),
        "a replayed alert nonce must be rejected, got {second:?}"
    );
}

// The end-to-end proof: what the send path queues is decodable + surfaced by
// the receive path (previously it was dropped). Bob configures an emergency
// broadcast with Alice as trusted contact and sends; Alice processes the queued
// disguised blob and gets an EmergencyAlertReceived-shaped outcome.
// @scenario: emergency_broadcast :: Alert delivery reaches the recipient
#[test]
fn emergency_broadcast_is_received_as_alert() {
    let (alice_wb, mut bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();

    bob_wb
        .configure_emergency_broadcast(vec![alice_contact_id.clone()], "check on me".into(), false)
        .expect("configure emergency broadcast");
    let result = bob_wb
        .send_emergency_broadcast()
        .expect("send emergency broadcast");
    assert_eq!(
        result.sent, 1,
        "the broadcast must queue one alert to Alice"
    );

    let pending = bob_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    let blob = &pending
        .iter()
        .find(|u| u.contact_id == alice_contact_id)
        .expect("a queued alert for Alice")
        .payload;

    let outcome = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        blob,
    )
    .expect("the queued emergency alert must be received, not dropped");
    match outcome {
        ReceiveOutcome::Alert(a) => {
            assert_eq!(a.kind, AlertKind::Emergency);
            assert_eq!(a.message, "check on me");
        }
        other => panic!("expected an Emergency alert outcome, got {other:?}"),
    }
}
