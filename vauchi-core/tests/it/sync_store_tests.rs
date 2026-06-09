// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
        storage.get_contact_last_sync("contact-a").unwrap(),
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
