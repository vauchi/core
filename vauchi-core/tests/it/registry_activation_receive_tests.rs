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
    tracker.record_push_sent([8u8; 32], 5);
    alice_wb
        .storage()
        .registry_activation()
        .save_activation(&bob_contact_id, &tracker)
        .unwrap();

    // Stale version, no echo — an ack answering an older registry version
    // than the one currently outstanding (a genuine in-flight crossing of a
    // registry change). Matching is on version, so this must not activate.
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

// @scenario: multi_device_sync :: Repeat acks never force replies from an active contact
// @internal
#[test]
fn received_echo_ack_after_activation_yields_no_reply() {
    // Review finding (CRITICAL): a malicious authenticated contact could
    // replay echo-carrying acks against the never-cleared outstanding push
    // and force an unbounded reply stream. The reply fires only on the
    // not-Active -> Active transition, mirroring the genesis handler.
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();

    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([8u8; 32], 4);
    tracker.record_ack(&[8u8; 32], 4).unwrap();
    alice_wb
        .storage()
        .registry_activation()
        .save_activation(&bob_contact_id, &tracker)
        .unwrap();

    let broadcast = bob_signed_broadcast(&bob_wb);
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
            .expect("repeat ack processed");
    match outcome {
        ReceiveOutcome::RegistryAckReceived { reply, .. } => {
            assert!(
                reply.is_none(),
                "an already-Active tracker must never be forced into a reply"
            );
        }
        other => panic!("expected RegistryAckReceived, got {other:?}"),
    }
}

// @scenario: multi_device_sync :: The ack reply echoes our registry exactly once
// @internal
#[test]
fn queue_registry_ack_echoes_own_registry_and_records_our_push_when_dormant() {
    // Bob is the ratchet initiator in this harness, so the queuing side is
    // Bob (a responder defers sends until it has received once).
    let (_alice_wb, bob_wb, _bob_contact_id, alice_contact_id) = setup_two_party();

    let reply = vauchi_core::api::RegistryReplyNeeded {
        sender_id: alice_contact_id.clone(),
        acked_version: 3,
        push_nonce: [6u8; 32],
        sender_device_id: [0u8; 32],
    };
    bob_wb.queue_registry_ack(&reply).unwrap();

    assert_eq!(
        bob_wb
            .storage()
            .pending()
            .count_pending_updates(&alice_contact_id)
            .unwrap(),
        1,
        "exactly one ack blob queued"
    );
    let tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker row");
    let (nonce, _version) = tracker
        .outstanding_push()
        .expect("echoing constitutes our own push");
    assert_eq!(nonce, [6u8; 32], "echo rides the same handshake nonce");
    assert_eq!(tracker.state(), ActivationState::Pushed);
}

// @scenario: multi_device_sync :: Concurrent handshakes never clobber an in-flight push
// @internal
#[test]
fn queue_registry_ack_keeps_an_outstanding_push_and_skips_the_echo() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();

    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 9);
    bob_wb
        .storage()
        .registry_activation()
        .save_activation(&alice_contact_id, &tracker)
        .unwrap();

    let reply = vauchi_core::api::RegistryReplyNeeded {
        sender_id: alice_contact_id.clone(),
        acked_version: 3,
        push_nonce: [6u8; 32],
        sender_device_id: [0u8; 32],
    };
    bob_wb.queue_registry_ack(&reply).unwrap();

    let tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker row");
    let (nonce, version) = tracker.outstanding_push().expect("still outstanding");
    assert_eq!(
        (nonce, version),
        ([1u8; 32], 9),
        "our own in-flight push survives the crossing handshake"
    );

    // Alice can open the queued blob and sees an ack WITHOUT an echo.
    let queued = bob_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(queued.len(), 1);
    let alice_identity_binding = alice_wb.identity().unwrap();
    let outcome = process_single_card_update(
        alice_identity_binding,
        alice_wb.storage(),
        &bob_contact_id,
        &queued[0].payload,
    )
    .expect("alice opens the ack");
    match outcome {
        ReceiveOutcome::RegistryAckReceived { reply, .. } => {
            assert!(reply.is_none(), "no echo when a push is already in flight");
        }
        other => panic!("expected RegistryAckReceived, got {other:?}"),
    }
}

// @scenario: multi_device_sync :: Activation re-arms the owed card so pre-activation edits deliver
// @internal
#[test]
fn activation_transition_rearms_the_owed_card_repropagation() {
    // e2e certification finding: a card edited BEFORE activation arms the
    // owed-repropagation marker, but every pre-activation tick fails
    // (session-less contact) and burns the retry budget — by the time the
    // handshake completes, the marker is backed off and the card never
    // reaches the peer. The activation transition must reset the budget:
    // the channel just came alive, and the peer's fleet needs our current
    // card.
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();

    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([8u8; 32], 4);
    alice_wb
        .storage()
        .registry_activation()
        .save_activation(&bob_contact_id, &tracker)
        .unwrap();
    // Marker exhausted by pre-activation failures.
    alice_wb
        .storage()
        .ux()
        .save_own_card_repropagate(&vauchi_core::types::OwnCardRepropagateState {
            needs_repropagate: true,
            failed_attempts: 250,
        })
        .unwrap();

    let broadcast = bob_signed_broadcast(&bob_wb);
    let ack =
        RegistryAckPayload::new([8u8; 32], 4, Some(broadcast.to_json().into_bytes())).unwrap();
    let blob = seal_from_bob(
        &bob_wb,
        &alice_contact_id,
        &VersionedPayload::encode_registry_ack(&ack),
    );
    let alice_identity = alice_wb.identity().unwrap();
    process_single_card_update(alice_identity, alice_wb.storage(), &bob_contact_id, &blob)
        .expect("activating ack");

    let marker = alice_wb.storage().ux().load_own_card_repropagate().unwrap();
    assert!(marker.needs_repropagate, "card push stays armed");
    assert_eq!(
        marker.failed_attempts, 0,
        "activation resets the retry budget so the owed card delivers"
    );
}

// @scenario: multi_device_sync :: A corrupt per-device session triggers handshake repair
// @internal
#[test]
fn device_session_decrypt_failure_demotes_activation_and_drops_the_session() {
    // Kimi condition / plan trigger 4: activation must cover session
    // REPAIR. A decrypt failure on an established device-scoped chain is
    // deterministic divergence — without repair, cards stall silently
    // forever: the blob is ACKed, the session stays corrupt, and the
    // scanner skips Active contacts so nothing ever re-handshakes.
    let (alice_wb, bob_wb, bob_contact_id, _alice_contact_id) = setup_two_party();
    let bob_device_id = *bob_wb.identity().unwrap().device_id();

    // Alice holds an Active handshake and a per-device session for Bob's
    // device — but the session has diverged (modeled with an unrelated
    // ratchet pair).
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([2u8; 32], 1);
    tracker.record_ack(&[2u8; 32], 1).unwrap();
    alice_wb
        .storage()
        .registry_activation()
        .save_activation(&bob_contact_id, &tracker)
        .unwrap();
    let unrelated = SymmetricKey::generate();
    let dh = X3DHKeyPair::generate();
    let alice_side = DoubleRatchetState::initialize_responder(&unrelated, dh);
    alice_wb
        .storage()
        .ratchets()
        .save_ratchet_state_for_device(&bob_contact_id, &bob_device_id, &alice_side, false)
        .unwrap();

    // Bob's send rides a chain Alice's stored session does not match.
    let divergent = SymmetricKey::generate();
    let other_dh = X3DHKeyPair::generate();
    let mut bob_side =
        DoubleRatchetState::initialize_initiator(&divergent, *other_dh.public_key()).unwrap();
    let blob = serde_json::to_vec(&bob_side.encrypt(b"card bytes").unwrap()).unwrap();

    let alice_identity = alice_wb.identity().unwrap();
    let result = vauchi_core::api::process_single_card_update_for_device(
        alice_identity,
        alice_wb.storage(),
        &bob_contact_id,
        &bob_device_id,
        &blob,
    );
    assert!(
        matches!(
            result,
            Err(vauchi_core::api::CardUpdateError::DecryptionFailed)
        ),
        "the blob itself still fails deterministically"
    );

    let tracker = alice_wb
        .storage()
        .registry_activation()
        .load_activation(&bob_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_ne!(
        tracker.state(),
        ActivationState::Active,
        "repair demotes activation so the scanner re-runs the handshake"
    );
    assert!(
        alice_wb
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&bob_contact_id, &bob_device_id)
            .unwrap()
            .is_none(),
        "the corrupt session row is dropped so the next receive re-bootstraps"
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
