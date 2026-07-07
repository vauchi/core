// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Aggregate-deletion completeness (problem
//! `2026-06-01-contact-delete-orphans`).
//!
//! Hard-deleting a contact must not leave relationship-scoped state behind.
//! Per-contact metadata that lives on the `contacts` row (nickname, avatar,
//! notes, CEK) drops with the row; `contact_field_notes` /
//! `contact_shared_names` / `contact_shared_avatars` cascade via FK. But
//! `contact_sync_timestamps`, `pending_updates`,
//! `contact_visibility_overrides`, and `dismissed_duplicates` have neither —
//! `delete_contact` must clear them explicitly. The live one: a stale
//! `contact_sync_timestamps` row is read at `sync/state.rs` with
//! `.unwrap_or(0)`, so a recurring `contact_id` (an exchanged contact's
//! fingerprint) would inherit a stale sync cursor and skip updates.

use vauchi_core::contact::Contact;
use vauchi_core::contact::kind::ImportSource;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{PendingUpdate, Storage, UpdateStatus};

fn open_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn make_contact(name: &str) -> Contact {
    let public_key = [name.as_bytes()[0]; 32];
    let card = ContactCard::new(name);
    Contact::from_exchange(public_key, card, SymmetricKey::generate(), 0)
}

// @internal
#[test]
fn hard_delete_clears_all_relationship_scoped_side_tables() {
    let storage = open_storage();
    let contact = make_contact("Alice");
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    // Populate every per-contact side table that neither cascades nor lives
    // on the contacts row.
    storage.sync().set_contact_last_sync(&id, 12_345).unwrap();
    storage
        .labels()
        .save_contact_override(&id, "phone", true)
        .unwrap();
    storage
        .pending()
        .queue_update(&PendingUpdate {
            id: "update-1".to_string(),
            contact_id: id.clone(),
            update_type: "card".to_string(),
            payload: vec![1, 2, 3],
            created_at: 0,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
        })
        .unwrap();
    storage
        .contacts()
        .dismiss_duplicate(&id, "other-contact")
        .unwrap();

    // Sanity: rows exist before deletion (guards against a vacuous test).
    assert_eq!(
        storage.sync().get_contact_last_sync(&id).unwrap(),
        Some(12_345)
    );
    assert!(
        !storage
            .labels()
            .load_contact_overrides(&id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(storage.pending().count_pending_updates(&id).unwrap(), 1);
    assert_eq!(
        storage
            .contacts()
            .load_dismissed_duplicates()
            .unwrap()
            .len(),
        1
    );

    // Hard delete the contact.
    assert!(
        storage.delete_contact(&id).unwrap(),
        "contact should have existed"
    );

    // Every relationship-scoped side table must now be empty for this contact.
    assert_eq!(
        storage.sync().get_contact_last_sync(&id).unwrap(),
        None,
        "contact_sync_timestamps must be cleared (stale last_sync wrongly gates sync on id reuse)"
    );
    assert!(
        storage
            .labels()
            .load_contact_overrides(&id)
            .unwrap()
            .is_empty(),
        "contact_visibility_overrides must be cleared"
    );
    assert_eq!(
        storage.pending().count_pending_updates(&id).unwrap(),
        0,
        "pending_updates must be cleared"
    );
    assert!(
        storage
            .contacts()
            .load_dismissed_duplicates()
            .unwrap()
            .is_empty(),
        "dismissed_duplicates referencing the contact must be cleared"
    );
}

/// The imported/exchanged distinction (HR-1) must be respected: an imported
/// contact holds no crypto/sync/relay state, so the exchange-only deletes in
/// `delete_contact` must be safe no-ops, while rows that *can* reference an
/// imported contact (a dismissed-duplicate pair) are still cleared.
// @internal
#[test]
fn hard_delete_imported_contact_cleans_its_rows_and_no_ops_exchange_only_tables() {
    let storage = open_storage();
    let contact = Contact::from_import(
        "contact-bob".to_string(),
        ContactCard::new("Bob"),
        ImportSource::VcardFile,
        None,
        0,
    );
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    // A duplicate suggestion can pair an imported contact with another contact.
    storage
        .contacts()
        .dismiss_duplicate(&id, "some-exchanged-id")
        .unwrap();
    assert_eq!(
        storage
            .contacts()
            .load_dismissed_duplicates()
            .unwrap()
            .len(),
        1
    );

    // HR-1: an imported contact has no sync cursor or queued updates — the
    // exchange-only tables are empty going in, so their deletes are no-ops.
    assert_eq!(storage.sync().get_contact_last_sync(&id).unwrap(), None);
    assert_eq!(storage.pending().count_pending_updates(&id).unwrap(), 0);

    assert!(
        storage.delete_contact(&id).unwrap(),
        "imported contact should have existed"
    );

    // Contact gone; its dismissed-duplicate pair cleared; the no-op deletes on
    // the exchange-only tables neither erred nor affected anything.
    assert!(
        storage.contacts().load_contact(&id).unwrap().is_none(),
        "imported contact must be removed"
    );
    assert!(
        storage
            .contacts()
            .load_dismissed_duplicates()
            .unwrap()
            .is_empty(),
        "dismissed_duplicates pair referencing the imported contact must be cleared"
    );
    assert_eq!(storage.sync().get_contact_last_sync(&id).unwrap(), None);
    assert_eq!(storage.pending().count_pending_updates(&id).unwrap(), 0);
}
