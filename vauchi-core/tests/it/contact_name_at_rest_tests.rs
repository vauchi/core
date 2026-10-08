// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A contact's display name is never stored in plaintext, and listing,
//! paging and search order by the decrypted name (vauchi/private#570).

use vauchi_core::ContactCard;
use vauchi_core::contact::Contact;
use vauchi_core::contact::kind::ImportSource;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::storage::Storage;

fn exchanged(seed: u8, name: &str) -> Contact {
    Contact::from_exchange(
        [seed; 32],
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

/// Stores "bob" and "Alice" (exchanged, no CEK), "carol" (exchanged, with a
/// CEK) and "Dave" (imported), in an order that is not alphabetical.
fn storage_with_mixed_contacts() -> Storage {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut carol = exchanged(3, "carol");
    carol.set_cek(ContentEncryptionKey::generate());
    let dave = Contact::from_import(
        "dave".into(),
        ContactCard::new("Dave"),
        ImportSource::VcardFile,
        None,
        0,
    );
    for contact in [carol, exchanged(2, "bob"), dave, exchanged(1, "Alice")] {
        storage.contacts().save_contact(&contact).unwrap();
    }
    storage
}

fn names(contacts: &[Contact]) -> Vec<&str> {
    contacts.iter().map(Contact::display_name).collect()
}

// @internal
#[test]
fn the_contacts_table_keeps_no_display_name() {
    let storage = storage_with_mixed_contacts();
    let conn = storage.connection();

    let mut stmt = conn.prepare("PRAGMA table_info(contacts)").unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let index_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'idx_contacts_display_name'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        !columns.contains(&"display_name".to_string()),
        "{columns:?}"
    );
    assert_eq!(index_count, 0);
}

// @internal
#[test]
fn listing_orders_every_kind_by_decrypted_name() {
    let storage = storage_with_mixed_contacts();

    let listed = storage.contacts().list_contacts().unwrap();

    assert_eq!(names(&listed), ["Alice", "bob", "carol", "Dave"]);
}

// @internal
#[test]
fn a_page_is_a_slice_of_the_decrypted_order() {
    let storage = storage_with_mixed_contacts();

    let page = storage.contacts().list_contacts_paginated(1, 2).unwrap();
    let past_the_end = storage.contacts().list_contacts_paginated(4, 2).unwrap();

    assert_eq!(names(&page), ["bob", "carol"]);
    assert!(past_the_end.is_empty());
}

// @internal
#[test]
fn search_matches_every_kind_case_insensitively_in_order() {
    let storage = storage_with_mixed_contacts();

    let found = storage.contacts().search_contacts("A").unwrap();

    assert_eq!(names(&found), ["Alice", "carol", "Dave"]);
}
