// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 slice 4a — genesis receive persists the carried registry (ADR-064
//! Amendment 2026-07-25, ADR-068 §Decision req 6).
//!
//! The genesis envelope already carries the sender's identity-signed
//! `RegistryBroadcast`; before F4 it was opened, verified, and discarded.
//! Persisting it (guarded, additive) is the cold-start front half: the
//! receiver can afterwards address the orphaned identity's devices, which
//! routing alone never allowed. Both guarded invariants stay untouched —
//! the `[0;32]` re-seat guard and the never-persisted initiator session.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::genesis::GenesisEnvelope;
use vauchi_core::identity::RegistryBroadcast;
use vauchi_core::network::mailbox_token::current_day_epoch;
use vauchi_core::sync::delta::VersionedPayload;
use vauchi_core::sync::registry_activation::ActivationState;
use vauchi_core::sync::safety_alert::{AlertKind, SafetyAlertPayload};

/// Bob holds Alice as an exchanged contact with NO session — the cold-start
/// state after Alice's exchanging device died and a sibling reaches out.
fn cold_start_world() -> (vauchi_core::Vauchi, vauchi_core::Vauchi, String, u64) {
    let alice_wb = create_vauchi_with_identity("Alice");
    let bob_wb = create_vauchi_with_identity("Bob");

    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let shared = SymmetricKey::generate();
    let alice_contact =
        Contact::from_exchange(alice_pk, ContactCard::new("Alice"), shared.clone(), 0);
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    let registry_version = alice_wb
        .identity()
        .unwrap()
        .initial_device_registry()
        .version();
    (alice_wb, bob_wb, alice_contact_id, registry_version)
}

fn genesis_alert_blob(
    alice_wb: &vauchi_core::Vauchi,
    bob_pk: &[u8; 32],
    shared: &SymmetricKey,
    nonce: [u8; 32],
) -> Vec<u8> {
    let alice_identity = alice_wb.identity().unwrap();
    let alert = SafetyAlertPayload::new(
        AlertKind::Duress,
        "help".into(),
        1_753_000_000,
        None,
        nonce,
        alice_identity,
        bob_pk,
    )
    .unwrap();
    let payload = VersionedPayload::encode_alert(&alert);
    let broadcast = RegistryBroadcast::new(
        &alice_identity.initial_device_registry(),
        alice_identity.signing_keypair(),
        1_753_000_000,
    );
    let (message, _session) = GenesisEnvelope::seal(
        shared,
        alice_identity,
        bob_pk,
        &broadcast,
        current_day_epoch(1_753_000_000),
        &payload,
    )
    .unwrap();
    serde_json::to_vec(&message).unwrap()
}

// @scenario: multi_device_sync :: A genesis alert seeds the sender's registry for activation
// @internal
#[test]
fn genesis_receive_persists_the_carried_registry_without_activating() {
    let (alice_wb, bob_wb, alice_contact_id, registry_version) = cold_start_world();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let shared = bob_wb
        .storage()
        .contacts()
        .load_contact(&alice_contact_id)
        .unwrap()
        .unwrap()
        .shared_key()
        .cloned()
        .unwrap();

    let blob = genesis_alert_blob(&alice_wb, &bob_pk, &shared, [4u8; 32]);
    let bob_identity = bob_wb.identity().unwrap();
    let outcome =
        process_single_card_update(bob_identity, bob_wb.storage(), &alice_contact_id, &blob)
            .expect("genesis alert processed");
    assert!(
        matches!(outcome, ReceiveOutcome::Alert(_)),
        "the alert still surfaces exactly as before"
    );

    assert!(
        !bob_wb
            .storage()
            .device()
            .load_contact_active_devices(&alice_contact_id)
            .unwrap()
            .is_empty(),
        "the carried registry is persisted — Bob can now address Alice's devices"
    );
    let tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(tracker.peer_version_held(), Some(registry_version));
    assert_ne!(
        tracker.state(),
        ActivationState::Active,
        "a genesis alert never activates Bob's send side (bilaterality)"
    );

    // Guarded invariant 1 still holds: the cold start persisted the [0;32]
    // responder session (no prior session existed), and no skip was counted.
    assert!(
        bob_wb
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&alice_contact_id, &[0u8; 32])
            .unwrap()
            .is_some()
    );
    assert_eq!(bob_wb.storage().genesis_limits().reseat_skips().unwrap(), 0);
}

// @scenario: multi_device_sync :: A stale carried registry never clobbers a newer held one
// @internal
#[test]
fn genesis_receive_tolerates_an_already_newer_held_registry() {
    let (alice_wb, bob_wb, alice_contact_id, registry_version) = cold_start_world();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let alice_identity = alice_wb.identity().unwrap();
    let shared = bob_wb
        .storage()
        .contacts()
        .load_contact(&alice_contact_id)
        .unwrap()
        .unwrap()
        .shared_key()
        .cloned()
        .unwrap();

    // Bob already holds Alice's registry (e.g. via an earlier vouched push).
    let held = RegistryBroadcast::new(
        &alice_identity.initial_device_registry(),
        alice_identity.signing_keypair(),
        1_753_000_100,
    );
    bob_wb
        .storage()
        .device()
        .save_contact_device_registry(
            &alice_contact_id,
            &held,
            alice_identity.signing_public_key(),
            u64::MAX,
        )
        .unwrap();

    let blob = genesis_alert_blob(&alice_wb, &bob_pk, &shared, [5u8; 32]);
    let bob_identity = bob_wb.identity().unwrap();
    let outcome =
        process_single_card_update(bob_identity, bob_wb.storage(), &alice_contact_id, &blob)
            .expect("same-version carried registry is an idempotent no-op");
    assert!(matches!(outcome, ReceiveOutcome::Alert(_)));

    let tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(tracker.peer_version_held(), Some(registry_version));
}
