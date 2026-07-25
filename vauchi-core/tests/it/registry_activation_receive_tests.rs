// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 handshake receive arms: RegistryPush (0x05) and RegistryAck (0x06)
//! arriving on the ratcheted contact channel (ADR-064 Amendment 2026-07-25).
//!
//! A received push persists the carried identity-signed broadcast
//! (monotonic-guarded), records the held peer version, and asks the caller
//! to reply with an ack; it never activates our send side (bilaterality —
//! only an ack of OUR push does that). A received ack activates when it
//! answers the outstanding push, tolerates stale/unknown acks without state
//! change, and persists a carried echo broadcast.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::RegistryBroadcast;
use vauchi_core::sync::delta::VersionedPayload;
use vauchi_core::sync::registry_activation::{
    ActivationState, ActivationTracker, RegistryAckPayload, RegistryPushPayload,
};

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

fn bob_signed_broadcast(bob_wb: &vauchi_core::Vauchi) -> RegistryBroadcast {
    let bob_identity = bob_wb.identity().unwrap();
    let registry = bob_identity.initial_device_registry();
    RegistryBroadcast::new(
        &registry,
        bob_identity.signing_keypair(),
        bob_wb.storage().clock().unix_seconds(),
    )
}

/// Seal a raw versioned payload with Bob's ratchet — the same wire envelope
/// as a card update (indistinguishable on the relay).
fn seal_from_bob(bob_wb: &vauchi_core::Vauchi, alice_contact_id: &str, payload: &[u8]) -> Vec<u8> {
    let (mut bob_ratchet, is_init) = bob_wb
        .storage()
        .ratchets()
        .load_ratchet_state(alice_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = bob_ratchet.encrypt(payload).unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(alice_contact_id, &bob_ratchet, is_init)
        .unwrap();
    serde_json::to_vec(&ratchet_msg).unwrap()
}

// @scenario: multi_device_sync :: A received registry push is persisted and answered, never trusted for sending
// @internal
#[test]
fn received_push_persists_registry_and_requests_ack_without_activating() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let broadcast = bob_signed_broadcast(&bob_wb);
    let version = broadcast.version();

    let push = RegistryPushPayload::new([3u8; 32], broadcast.to_json().into_bytes()).unwrap();
    let blob = seal_from_bob(
        &bob_wb,
        &alice_contact_id,
        &VersionedPayload::encode_registry_push(&push),
    );

    let alice_identity = alice_wb.identity().unwrap();
    let outcome =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob)
            .expect("push processed");

    match outcome {
        ReceiveOutcome::RegistryPushReceived(reply) => {
            assert_eq!(reply.sender_id, bob_contact_id);
            assert_eq!(reply.acked_version, version);
            assert_eq!(reply.push_nonce, [3u8; 32]);
        }
        other => panic!("expected RegistryPushReceived, got {other:?}"),
    }

    assert!(
        !alice_wb
            .storage()
            .device()
            .load_contact_active_devices(&bob_contact_id)
            .unwrap()
            .is_empty(),
        "Bob's registry is persisted"
    );
    let tracker = alice_wb
        .storage()
        .registry_activation()
        .load_activation(&bob_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(tracker.peer_version_held(), Some(version));
    assert_ne!(
        tracker.state(),
        ActivationState::Active,
        "receiving a push must never activate our send side (bilaterality)"
    );
}

// @scenario: multi_device_sync :: An ack of our outstanding push activates per-device sending
// @internal
#[test]
fn received_matching_ack_activates_and_persists_the_echo() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();

    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([8u8; 32], 4);
    alice_wb
        .storage()
        .registry_activation()
        .save_activation(&bob_contact_id, &tracker)
        .unwrap();

    let broadcast = bob_signed_broadcast(&bob_wb);
    let bob_version = broadcast.version();
    let ack =
        RegistryAckPayload::new([8u8; 32], 4, Some(broadcast.to_json().into_bytes())).unwrap();
    let blob = seal_from_bob(
        &bob_wb,
        &alice_contact_id,
        &VersionedPayload::encode_registry_ack(&ack),
    );

    let alice_identity = alice_wb.identity().unwrap();
    let outcome =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob)
            .expect("ack processed");

    match outcome {
        ReceiveOutcome::RegistryAckReceived { sender_id, reply } => {
            assert_eq!(sender_id, bob_contact_id);
            let reply = reply.expect("echo requires a confirming reply");
            assert_eq!(reply.acked_version, bob_version);
            assert_eq!(reply.push_nonce, [8u8; 32]);
        }
        other => panic!("expected RegistryAckReceived, got {other:?}"),
    }

    let tracker = alice_wb
        .storage()
        .registry_activation()
        .load_activation(&bob_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(tracker.state(), ActivationState::Active);
    assert_eq!(tracker.peer_version_held(), Some(bob_version));
    assert!(
        !alice_wb
            .storage()
            .device()
            .load_contact_active_devices(&bob_contact_id)
            .unwrap()
            .is_empty(),
        "the echoed registry is persisted"
    );
}

// @scenario: multi_device_sync :: Stale or unknown acks are tolerated without state change
// @internal
#[test]
fn received_mismatched_ack_is_tolerated_without_state_change() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();

    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([8u8; 32], 4);
    alice_wb
        .storage()
        .registry_activation()
        .save_activation(&bob_contact_id, &tracker)
        .unwrap();

    // Wrong nonce, no echo — e.g. an ack crossing a registry change.
    let ack = RegistryAckPayload::new([9u8; 32], 4, None).unwrap();
    let blob = seal_from_bob(
        &bob_wb,
        &alice_contact_id,
        &VersionedPayload::encode_registry_ack(&ack),
    );

    let alice_identity = alice_wb.identity().unwrap();
    let outcome =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob)
            .expect("mismatched ack is not an error blob");

    match outcome {
        ReceiveOutcome::RegistryAckReceived { reply, .. } => {
            assert!(reply.is_none(), "nothing to confirm without an echo");
        }
        other => panic!("expected RegistryAckReceived, got {other:?}"),
    }

    let tracker = alice_wb
        .storage()
        .registry_activation()
        .load_activation(&bob_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(
        tracker.state(),
        ActivationState::Pushed,
        "a mismatched ack must not activate or demote"
    );
}

// @scenario: multi_device_sync :: A forged registry push is rejected without persisting anything
// @internal
#[test]
fn received_push_with_forged_broadcast_fails_closed() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();

    // Signed by Mallory, not by Bob's identity key the contact pinned.
    let mallory = create_vauchi_with_identity("Mallory");
    let broadcast = bob_signed_broadcast(&mallory);

    let push = RegistryPushPayload::new([3u8; 32], broadcast.to_json().into_bytes()).unwrap();
    let blob = seal_from_bob(
        &bob_wb,
        &alice_contact_id,
        &VersionedPayload::encode_registry_push(&push),
    );

    let alice_identity = alice_wb.identity().unwrap();
    let result =
        process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob);

    assert!(result.is_err(), "forged broadcast must be rejected");
    assert!(
        alice_wb
            .storage()
            .device()
            .load_contact_active_devices(&bob_contact_id)
            .unwrap()
            .is_empty(),
        "nothing persisted from a forged push"
    );
    assert!(
        alice_wb
            .storage()
            .registry_activation()
            .load_activation(&bob_contact_id)
            .unwrap()
            .is_none(),
        "no tracker state from a forged push"
    );
}
