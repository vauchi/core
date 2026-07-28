// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 push scanner: the sync cycle queues a RegistryPush wherever the peer
//! has not confirmed our *current* registry version (ADR-064 Amendment
//! 2026-07-25, owner-proposed vouched push).
//!
//! Level-triggered by design: device-link completion, exchange completion,
//! and registry changes all leave `own version != acked version`, and the
//! scanner reconciles that state on the next sync tick — so a crash between
//! any trigger and its push self-heals, and the three trigger call sites
//! collapse into one convergence loop.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::{DeviceRegistry, Identity, RegistryBroadcast};
use vauchi_core::storage::{PendingUpdate, UpdateStatus};
use vauchi_core::sync::registry_activation::{ActivationState, ActivationTracker};

/// Build a three-device peer (Alice) as a signed registry broadcast — the
/// state a surviving sibling inherits via owner-sync, holding the peer's
/// device ids without any per-device session yet.
fn three_device_peer(now: u64) -> (RegistryBroadcast, [u8; 32], Vec<[u8; 32]>) {
    let seed = [42u8; 32];
    let identities: Vec<Identity> = (0..3)
        .map(|index| {
            Identity::from_device_link(
                seed,
                "Alice".into(),
                index,
                format!("Alice device {}", index + 1),
                1,
            )
        })
        .collect();
    let first = &identities[0];
    let mut registry = DeviceRegistry::new(
        first.device_info().to_registered(&seed),
        first.signing_keypair(),
    );
    for identity in identities.iter().skip(1) {
        registry
            .add_device(
                identity.device_info().to_registered(&seed),
                first.signing_keypair(),
            )
            .unwrap();
    }
    let broadcast = RegistryBroadcast::new(&registry, first.signing_keypair(), now);
    let signing_pk = *first.signing_public_key();
    let device_ids = identities.iter().map(|i| *i.device_id()).collect();
    (broadcast, signing_pk, device_ids)
}

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

// @scenario: multi_device_sync :: The sync cycle pushes our registry to unconfirmed contacts
// @internal
#[test]
fn scanner_queues_initial_push_that_the_peer_can_open() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();

    let queued = bob_wb.queue_registry_pushes().unwrap();
    assert_eq!(queued, 1, "one dormant contact gets one push");

    let tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(tracker.state(), ActivationState::Pushed);

    let pending = bob_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(pending.len(), 1);
    let alice_identity = alice_wb.identity().unwrap();
    let outcome = process_single_card_update(
        alice_identity,
        alice_wb.storage(),
        &bob_contact_id,
        &pending[0].payload,
    )
    .expect("alice opens the push");
    match outcome {
        ReceiveOutcome::RegistryPushReceived(reply) => {
            assert_eq!(reply.sender_id, bob_contact_id);
        }
        other => panic!("expected RegistryPushReceived, got {other:?}"),
    }
}

// @scenario: multi_device_sync :: The bootstrap push is a single identity-mailbox cold-start
// @internal
#[test]
fn scanner_queues_a_single_cold_start_push_even_when_peer_devices_are_known() {
    // The lost-primary bootstrap: A2 knows Bob's whole device registry
    // (owner-synced from the dead A1) but Bob's devices do NOT yet know A2. A
    // device-scoped push — routed to a peer device's mailbox and/or signed
    // with A2's device token — is unresolvable to those peers and dead-letters
    // (2026-07-26 root cause). The bootstrap push must stay a SINGLE cold-start
    // to the peer's identity mailbox (`target_device_id: None`): any peer
    // device drains it, learns A2's registry, and its ack routes back
    // device-scoped. Reverts the Slice 5b per-device fan-out.
    let bob_wb = create_vauchi_with_identity("Bob");
    let now = bob_wb.storage().clock().unix_seconds();
    let (peer_broadcast, peer_pk, _peer_device_ids) = three_device_peer(now);

    let contact = Contact::from_exchange(
        peer_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    bob_wb.add_contact(contact).unwrap();
    bob_wb
        .storage()
        .device()
        .save_contact_device_registry(&contact_id, &peer_broadcast, &peer_pk, 60)
        .unwrap();

    bob_wb.queue_registry_pushes().unwrap();

    let pending = bob_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(
        pending.len(),
        1,
        "the bootstrap push is a single identity-mailbox cold-start, not per-device"
    );
    assert_eq!(
        pending[0].target_device_id, None,
        "the cold-start push rides the peer's identity mailbox so any peer device can drain it"
    );
}

// @scenario: multi_device_sync :: The ack routes back to the originating device
// @internal
#[test]
fn ack_routes_to_the_originating_device_scoped_mailbox() {
    // The reverse leg of the lost-primary bootstrap: a peer replying to a
    // surviving sibling's push must route the ack to THAT sibling's
    // device-scoped mailbox, or a shared identity mailbox lets another
    // sibling drain it and the pushing sibling never reaches Active
    // (ADR-064 Amendment 2026-07-25).
    let bob_wb = create_vauchi_with_identity("Bob");
    let now = bob_wb.storage().clock().unix_seconds();
    let (_broadcast, peer_pk, peer_device_ids) = three_device_peer(now);

    let contact = Contact::from_exchange(
        peer_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    bob_wb.add_contact(contact).unwrap();

    let origin = peer_device_ids[1];
    let reply = vauchi_core::api::sync::RegistryReplyNeeded {
        sender_id: contact_id,
        acked_version: 1,
        push_nonce: [9u8; 32],
        sender_device_id: origin,
    };
    bob_wb.queue_registry_ack(&reply).unwrap();

    let pending = bob_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].target_device_id,
        Some(origin),
        "the ack routes to the originating device's device-scoped mailbox"
    );
}

// @scenario: multi_device_sync :: A confirmed contact is not re-pushed
// @internal
#[test]
fn scanner_skips_contacts_whose_current_version_is_acked() {
    let (_alice_wb, bob_wb, _bob_contact_id, alice_contact_id) = setup_two_party();

    let own_version = bob_wb
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .map(|r| r.version())
        .unwrap_or(1);
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], own_version);
    tracker.record_ack(&[1u8; 32], own_version).unwrap();
    bob_wb
        .storage()
        .registry_activation()
        .save_activation(&alice_contact_id, &tracker)
        .unwrap();

    assert_eq!(bob_wb.queue_registry_pushes().unwrap(), 0);
    assert_eq!(
        bob_wb
            .storage()
            .pending()
            .count_pending_updates(&alice_contact_id)
            .unwrap(),
        0
    );
}

// @scenario: multi_device_sync :: A registry change re-pushes until re-confirmed
// @internal
#[test]
fn scanner_repushes_when_acked_version_is_stale() {
    let (_alice_wb, bob_wb, _bob_contact_id, alice_contact_id) = setup_two_party();

    // The peer confirmed an OLDER version than our current registry (e.g.
    // a device was linked since) — the scanner must re-push.
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 0);
    tracker.record_ack(&[1u8; 32], 0).unwrap();
    bob_wb
        .storage()
        .registry_activation()
        .save_activation(&alice_contact_id, &tracker)
        .unwrap();

    assert_eq!(bob_wb.queue_registry_pushes().unwrap(), 1);
    let tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(
        tracker.state(),
        ActivationState::Pushed,
        "stale ack demotes until the new version is confirmed"
    );
}

// @scenario: multi_device_sync :: Blocked contacts receive nothing, silently
// @internal
#[test]
fn scanner_skips_blocked_contacts() {
    let (_alice_wb, bob_wb, _bob_contact_id, alice_contact_id) = setup_two_party();

    bob_wb.block_contact(&alice_contact_id).unwrap();
    assert_eq!(bob_wb.queue_registry_pushes().unwrap(), 0);
    assert_eq!(
        bob_wb
            .storage()
            .pending()
            .count_pending_updates(&alice_contact_id)
            .unwrap(),
        0,
        "ADR-056: a blocked contact receives nothing"
    );
}

// @scenario: multi_device_sync :: One handshake message in flight per contact
// @internal
#[test]
fn scanner_keeps_one_push_in_flight_per_contact() {
    let (_alice_wb, bob_wb, _bob_contact_id, alice_contact_id) = setup_two_party();

    assert_eq!(bob_wb.queue_registry_pushes().unwrap(), 1);
    assert_eq!(
        bob_wb.queue_registry_pushes().unwrap(),
        0,
        "a queued, undelivered push must not be duplicated"
    );
    assert_eq!(
        bob_wb
            .storage()
            .pending()
            .count_pending_updates(&alice_contact_id)
            .unwrap(),
        1
    );
}

// @scenario: multi_device_sync :: A queued card update cannot starve registry activation
// @internal
#[test]
fn scanner_queues_activation_alongside_a_blocked_card_update() {
    let (_alice_wb, bob_wb, _bob_contact_id, alice_contact_id) = setup_two_party();
    bob_wb
        .storage()
        .pending()
        .queue_update(&PendingUpdate {
            id: "blocked-card".to_string(),
            contact_id: alice_contact_id.clone(),
            update_type: "card_delta".to_string(),
            payload: vec![1, 2, 3],
            created_at: 0,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
            target_device_id: None,
        })
        .unwrap();

    assert_eq!(
        bob_wb.queue_registry_pushes().unwrap(),
        1,
        "a card update that needs activation must not block the handshake that unlocks it"
    );
    let pending = bob_wb
        .storage()
        .pending()
        .get_pending_updates(&alice_contact_id)
        .unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(
        pending
            .iter()
            .filter(|update| update.update_type == "registry_handshake")
            .count(),
        1,
        "the scanner still keeps only one handshake in flight"
    );
}
