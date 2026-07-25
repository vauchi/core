// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 activation persistence (migration v65).
//!
//! One row per contact snapshots the [`ActivationTracker`]; a missing row is
//! the tracker default (`Dormant` — exactly today's shipped behavior, which
//! is what makes slices 2-4 revertible). No secret material: nonces are
//! correlation values and versions are counters, matching the
//! `genesis_decrypt_contact_limits` plaintext-FK convention.

use vauchi_core::Storage;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::registry_activation::{ActivationState, ActivationTracker};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn saved_contact(storage: &Storage, seed: u8, name: &str) -> String {
    let contact = Contact::from_exchange(
        [seed; 32],
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    id
}

// @internal
#[test]
fn missing_row_loads_as_none() {
    let storage = test_storage();
    let contact_id = saved_contact(&storage, 1, "Alice");
    let loaded = storage
        .registry_activation()
        .load_activation(&contact_id)
        .unwrap();
    assert!(loaded.is_none(), "no handshake history yet");
}

// @internal
#[test]
fn tracker_roundtrips_through_storage_in_every_state() {
    let storage = test_storage();
    let contact_id = saved_contact(&storage, 2, "Bob");
    let store = storage.registry_activation();

    // Pushed.
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([5u8; 32], 3);
    tracker.record_peer_registry(2);
    store.save_activation(&contact_id, &tracker).unwrap();
    let loaded = store.load_activation(&contact_id).unwrap().expect("row");
    assert_eq!(loaded.state(), ActivationState::Pushed);
    assert_eq!(loaded.peer_version_held(), Some(2));
    assert_eq!(loaded.our_version_acked(), None);

    // Active — and the reloaded tracker must accept a duplicate matching
    // ack (outstanding push survives persistence).
    tracker.record_ack(&[5u8; 32], 3).unwrap();
    store.save_activation(&contact_id, &tracker).unwrap();
    let mut loaded = store.load_activation(&contact_id).unwrap().expect("row");
    assert_eq!(loaded.state(), ActivationState::Active);
    assert_eq!(loaded.our_version_acked(), Some(3));
    loaded.record_ack(&[5u8; 32], 3).expect("idempotent re-ack");

    // Back to nothing after the peer empties its registry.
    loaded.record_peer_registry_emptied();
    store.save_activation(&contact_id, &loaded).unwrap();
    let reloaded = store.load_activation(&contact_id).unwrap().expect("row");
    assert_eq!(reloaded.state(), ActivationState::Dormant);
    assert_eq!(reloaded.peer_version_held(), None);
}

// @internal
#[test]
fn save_is_per_contact() {
    let storage = test_storage();
    let alice = saved_contact(&storage, 3, "Alice");
    let bob = saved_contact(&storage, 4, "Bob");
    let store = storage.registry_activation();

    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([9u8; 32], 1);
    store.save_activation(&alice, &tracker).unwrap();

    assert!(store.load_activation(&bob).unwrap().is_none());
    assert_eq!(
        store.load_activation(&alice).unwrap().expect("row").state(),
        ActivationState::Pushed
    );
}

// @internal
#[test]
fn deleting_the_contact_cascades_the_activation_row() {
    let storage = test_storage();
    let contact_id = saved_contact(&storage, 5, "Ephemeral");
    let store = storage.registry_activation();

    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 1);
    store.save_activation(&contact_id, &tracker).unwrap();

    storage.delete_contact(&contact_id).unwrap();
    assert!(
        storage
            .registry_activation()
            .load_activation(&contact_id)
            .unwrap()
            .is_none(),
        "activation row must not outlive its contact (FK cascade)"
    );
}

// @internal
#[test]
fn negative_stored_versions_fail_closed_as_corruption() {
    // Review finding F4: the table is written only by this store, so a
    // negative version is tampering or corruption — surface it, never
    // silently clamp (consistent with the half-present-push policy).
    let storage = test_storage();
    let contact_id = saved_contact(&storage, 6, "Corrupt");
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 1);
    storage
        .registry_activation()
        .save_activation(&contact_id, &tracker)
        .unwrap();
    storage
        .connection()
        .execute(
            "UPDATE registry_activation SET pushed_version = -3 WHERE contact_id = ?1",
            rusqlite::params![contact_id],
        )
        .unwrap();

    assert!(
        storage
            .registry_activation()
            .load_activation(&contact_id)
            .is_err(),
        "a negative stored version must load as corruption, not clamp to 0"
    );
}

// @internal
#[test]
fn saving_for_unknown_contact_fails_closed() {
    let storage = test_storage();
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 1);
    assert!(
        storage
            .registry_activation()
            .save_activation("no-such-contact", &tracker)
            .is_err(),
        "FK must reject orphan activation rows"
    );
}
