// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tests for the sync-domain persistence view (`SyncStore`).
//!
//! Decision (a) of problem `2026-06-09-storage-per-domain-store-boundaries`:
//! `SyncStore` owns every sync-shaped table, including `contact_sync_timestamps`
//! whose rows key off the contact domain. These tests exercise the scoped view
//! directly and the cross-domain cleanup it now owns.

use vauchi_core::{Storage, SymmetricKey, SyncStore};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// A consumer scoped to the sync domain receives only `&SyncStore` — it is
// statically unable to reach contacts, identity, or any other table.
fn last_sync_via_scoped_view(store: &SyncStore<'_>, contact_id: &str) -> Option<u64> {
    store.get_contact_last_sync(contact_id).unwrap()
}

// @internal
#[test]
fn test_sync_store_contact_last_sync_roundtrip() {
    let storage = test_storage();

    assert_eq!(
        last_sync_via_scoped_view(&storage.sync(), "contact-a"),
        None
    );

    storage
        .sync()
        .set_contact_last_sync("contact-a", 4242)
        .unwrap();

    assert_eq!(
        last_sync_via_scoped_view(&storage.sync(), "contact-a"),
        Some(4242)
    );
    // Same row is visible through the legacy forwarding API — one connection.
    assert_eq!(
        storage.sync().get_contact_last_sync("contact-a").unwrap(),
        Some(4242)
    );
}

// @internal
#[test]
fn test_sync_store_forget_contact_clears_timestamp() {
    let storage = test_storage();
    storage
        .sync()
        .set_contact_last_sync("contact-b", 99)
        .unwrap();

    storage.sync().forget_contact("contact-b").unwrap();

    assert_eq!(
        storage.sync().get_contact_last_sync("contact-b").unwrap(),
        None
    );
}

// @internal
#[test]
fn test_sync_store_version_vector_default_is_none() {
    let storage = test_storage();
    assert!(storage.sync().load_version_vector().unwrap().is_none());
}

// @internal
#[test]
fn test_sync_store_field_timestamps_default_is_empty() {
    let storage = test_storage();
    assert!(storage.sync().load_field_timestamps().unwrap().is_empty());
}

// @internal
#[test]
fn peer_card_ordering_migration_preserves_a_v90_contact() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("ordering.db");
    let key = SymmetricKey::generate();
    let contact = crate::common::device_sync::create_test_contact("Before upgrade");
    let id = contact.id().to_string();
    {
        let storage = Storage::open(&database, key.clone()).unwrap();
        storage.contacts().save_contact(&contact).unwrap();
        storage.connection().execute_batch(
            "DROP TABLE IF EXISTS peer_card_ordering; DELETE FROM schema_version WHERE version > 90;",
        ).unwrap();
    }

    let upgraded = Storage::open(&database, key).unwrap();
    let table_count: u64 = upgraded.connection().query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'peer_card_ordering'",
        [], |row| row.get(0),
    ).unwrap();

    assert_eq!(
        table_count, 1,
        "upgrade must install durable ordering storage"
    );
    assert_eq!(
        upgraded
            .contacts()
            .load_contact(&id)
            .unwrap()
            .unwrap()
            .display_name(),
        "Before upgrade"
    );
}

// @internal
#[test]
fn peer_card_ordering_round_trips_and_is_deleted_with_its_contact() {
    let storage = test_storage();
    let contact = crate::common::device_sync::create_test_contact("Ordering");
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    assert_eq!(
        storage.contacts().test_load_card_ordering(&id).unwrap(),
        None
    );

    storage
        .contacts()
        .test_save_card_ordering(&id, b"ordering-state")
        .unwrap();

    assert_eq!(
        storage.contacts().test_load_card_ordering(&id).unwrap(),
        Some(b"ordering-state".to_vec())
    );
    assert_eq!(
        storage
            .contacts()
            .test_load_card_ordering("different-contact")
            .unwrap(),
        None
    );
    let encrypted: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT state_encrypted FROM peer_card_ordering WHERE contact_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(encrypted, b"ordering-state");
    storage.delete_contact(&id).unwrap();
    assert_eq!(
        storage.contacts().test_load_card_ordering(&id).unwrap(),
        None
    );
}

// @internal
#[test]
fn peer_card_ordering_survives_storage_key_rotation() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("rotation.db");
    let old_key = SymmetricKey::generate();
    let new_key = SymmetricKey::generate();
    let contact = crate::common::device_sync::create_test_contact("Rotated");
    let id = contact.id().to_string();
    {
        let mut storage = Storage::open(&database, old_key.clone()).unwrap();
        storage.contacts().save_contact(&contact).unwrap();
        storage
            .contacts()
            .test_save_card_ordering(&id, b"persistent-ordering")
            .unwrap();
        storage.rekey(new_key.clone()).unwrap();
        assert_eq!(
            storage.contacts().test_load_card_ordering(&id).unwrap(),
            Some(b"persistent-ordering".to_vec())
        );
    }

    let storage = Storage::open(&database, new_key).unwrap();
    assert_eq!(
        storage.contacts().test_load_card_ordering(&id).unwrap(),
        Some(b"persistent-ordering".to_vec())
    );
    assert!(matches!(
        Storage::open(&database, old_key),
        Err(vauchi_core::storage::StorageError::WrongKey)
    ));
}
