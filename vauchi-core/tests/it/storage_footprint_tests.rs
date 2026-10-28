// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A hundred contacts fit in a small database, and the file gives the space
//! back when they are deleted.

use std::path::Path;

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::Storage;

const MB: u64 = 1024 * 1024;

fn on_disk(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum()
}

fn checkpoint(storage: &Storage) {
    storage
        .connection()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
}

// @scenario: performance :: Reasonable storage footprint
#[test]
fn a_hundred_contacts_stay_far_under_50_mb_and_deleting_them_shrinks_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(&dir.path().join("vauchi.db"), SymmetricKey::generate()).unwrap();
    let mut ids = Vec::new();
    for i in 0..100 {
        let name = format!("Contact {i}");
        let mut card = ContactCard::new(&name);
        for (kind, label, value) in [
            (
                FieldType::Phone,
                "Mobile",
                format!("+41 79 000 {i:02} {i:02}"),
            ),
            (FieldType::Email, "Work", format!("contact{i}@example.com")),
            (
                FieldType::Address,
                "Home",
                format!("{i} Long Street, 8000 Zurich"),
            ),
        ] {
            card.add_field(ContactField::new(kind, label, &value, 0))
                .unwrap();
        }
        let peer = Identity::create(&name, 0);
        let contact = Contact::from_exchange(
            *peer.signing_public_key(),
            card,
            SymmetricKey::generate(),
            0,
        );
        ids.push(contact.id().to_string());
        storage.contacts().save_contact(&contact).unwrap();
    }
    checkpoint(&storage);
    let with_contacts = on_disk(dir.path());

    for id in &ids {
        storage.delete_contact(id).unwrap();
    }
    checkpoint(&storage);
    let after_deleting = on_disk(dir.path());

    assert!(with_contacts < 50 * MB, "{with_contacts} bytes");
    assert!(
        after_deleting < with_contacts,
        "{after_deleting} bytes after deleting, {with_contacts} before"
    );
}
