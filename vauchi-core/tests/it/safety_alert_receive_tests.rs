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
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, VauchiEvent, process_single_card_update};
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

// ── Durable alert facts (delivery-axis findings, ─────────────────────
//    2026-07-21-per-device-ratchet-registry-dormant): accepting an alert
//    burns its replay nonce, so the alert must be durable from the same
//    transaction — a crash before surfacing must not lose it.

// @internal
#[test]
fn received_alert_persists_unsurfaced_fact_atomically() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let bob_signing_pk = *bob_wb.identity().unwrap().signing_public_key();

    let blob = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Duress,
        [11u8; 32],
    );
    process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &blob,
    )
    .expect("alert must be received");

    let facts = alice_wb
        .storage()
        .safety_alerts()
        .load_unsurfaced_facts()
        .unwrap();
    assert_eq!(facts.len(), 1, "the accepted alert must be a durable fact");
    assert_eq!(facts[0].contact_id, bob_contact_id);
    assert_eq!(facts[0].nonce, [11u8; 32]);

    // The stored bytes are the exact signed wire payload: decodable and
    // re-verifiable against the sender+recipient keys (sibling fan-out
    // depends on this).
    match VersionedPayload::decode(&facts[0].signed_payload).unwrap() {
        VersionedPayload::Alert(stored) => {
            assert_eq!(stored.kind(), AlertKind::Duress);
            assert_eq!(stored.message(), "help me");
            assert_eq!(stored.timestamp(), 1_720_000_000);
            assert_eq!(stored.nonce(), &[11u8; 32]);
            assert!(stored.verify(&bob_signing_pk, &alice_signing_pk));
        }
        other => panic!("stored fact must decode as an Alert, got {other:?}"),
    }
}

// @internal
#[test]
fn replayed_alert_does_not_duplicate_fact() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    let blob1 = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Duress,
        [12u8; 32],
    );
    let blob2 = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Duress,
        [12u8; 32],
    );

    let alice_identity = alice_wb.identity().unwrap();
    process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob1)
        .expect("first alert must be received");
    let second =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob2);
    assert!(matches!(second, Err(CardUpdateError::ReplayDetected)));

    let facts = alice_wb
        .storage()
        .safety_alerts()
        .load_unsurfaced_facts()
        .unwrap();
    assert_eq!(facts.len(), 1, "a replay must not duplicate the fact");
}

// @internal
#[test]
fn alert_receive_is_atomic_fact_failure_preserves_the_nonce() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    let blob = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Duress,
        [13u8; 32],
    );

    // Sabotage the fact table so the insert fails mid-transaction.
    alice_wb
        .storage()
        .connection()
        .execute_batch("ALTER TABLE safety_alert_facts RENAME TO safety_alert_facts_gone")
        .unwrap();

    let alice_identity = alice_wb.identity().unwrap();
    let failed =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob);
    assert!(
        failed.is_err(),
        "a fact-persistence failure must fail the receive, got {failed:?}"
    );

    // Restore the table: the SAME blob must now succeed — the failed attempt
    // must not have burned the replay nonce or advanced the stored ratchet.
    alice_wb
        .storage()
        .connection()
        .execute_batch("ALTER TABLE safety_alert_facts_gone RENAME TO safety_alert_facts")
        .unwrap();
    let retried =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob);
    assert!(
        matches!(retried, Ok(ReceiveOutcome::Alert(_))),
        "retry after a rolled-back failure must succeed, got {retried:?}"
    );
    assert_eq!(
        alice_wb
            .storage()
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .len(),
        1
    );
}

// @internal
#[test]
fn surfacing_dispatches_pending_facts_at_least_once() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    let blob = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Duress,
        [21u8; 32],
    );
    // The storage-level receive persists the fact but dispatches nothing —
    // surfacing must derive from the durable fact, not the in-memory outcome.
    process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &blob,
    )
    .expect("alert must be received");

    let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = collected.clone();
    alice_wb.add_event_handler(std::sync::Arc::new(move |e| {
        sink.lock().unwrap().push(e);
    }));

    let n = alice_wb.surface_pending_safety_alerts().unwrap();
    assert_eq!(n, 1, "one pending fact must be dispatched");
    {
        let events = collected.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            VauchiEvent::DuressAlertReceived {
                contact_id,
                message,
                timestamp,
                location,
                alert_nonce,
            } => {
                assert_eq!(contact_id, &bob_contact_id);
                assert_eq!(message, "help me");
                assert_eq!(*timestamp, 1_720_000_000);
                assert_eq!(*location, None);
                assert_eq!(alert_nonce, &[21u8; 32]);
            }
            other => panic!("expected DuressAlertReceived, got {other:?}"),
        }
    }

    // At-least-once until a presentation acknowledgement exists (platform
    // follow-up): a second pass re-dispatches; consumers dedup on the nonce.
    let again = alice_wb.surface_pending_safety_alerts().unwrap();
    assert_eq!(again, 1, "facts stay pending until acknowledged");
    assert_eq!(collected.lock().unwrap().len(), 2);
}

// ADR-056: blocking is fully silent — no notifications about blocked users.
// A durable fact received before the block must not be presented while the
// contact is blocked (the fact itself remains for when/if they are unblocked).
// @internal
#[test]
fn blocked_contact_alert_is_not_surfaced() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    let blob = create_alert_blob(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        AlertKind::Duress,
        [31u8; 32],
    );
    process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &blob,
    )
    .expect("alert must be received");

    let mut bob = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    bob.set_blocked(true);
    alice_wb.storage().contacts().save_contact(&bob).unwrap();

    let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = collected.clone();
    alice_wb.add_event_handler(std::sync::Arc::new(move |e| {
        sink.lock().unwrap().push(e);
    }));

    let n = alice_wb.surface_pending_safety_alerts().unwrap();
    assert_eq!(n, 0, "a blocked contact's alert must not surface (ADR-056)");
    assert!(collected.lock().unwrap().is_empty());
}

// A fact whose stored payload carries a different signed nonce than its row
// key is not a verified-receive artifact — surfacing must skip it rather than
// dispatch an event whose identity does not match its evidence.
// @internal
#[test]
fn nonce_mismatched_fact_is_not_surfaced() {
    let (alice_wb, bob_wb, bob_contact_id, _alice_contact_id) = setup_two_party();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    let bob_identity = bob_wb.identity().unwrap();
    let alert = SafetyAlertPayload::new(
        AlertKind::Duress,
        "help me".into(),
        1_720_000_000,
        None,
        [41u8; 32],
        bob_identity,
        &alice_signing_pk,
    )
    .unwrap();
    let payload = VersionedPayload::encode_alert(&alert);

    // Row nonce differs from the signed nonce inside the payload.
    alice_wb
        .storage()
        .safety_alerts()
        .insert_fact_if_absent(&bob_contact_id, &[42u8; 32], &payload, 80)
        .unwrap();

    let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = collected.clone();
    alice_wb.add_event_handler(std::sync::Arc::new(move |e| {
        sink.lock().unwrap().push(e);
    }));

    let n = alice_wb.surface_pending_safety_alerts().unwrap();
    assert_eq!(n, 0, "a nonce-mismatched fact must be skipped");
    assert!(collected.lock().unwrap().is_empty());
}
