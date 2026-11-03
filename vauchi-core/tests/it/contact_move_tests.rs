// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Moving a contact to a new id when it accepts a recovery
//! (vauchi/private#796, step 5b): a contact's id is the hex of its key, so
//! the rekey moves it, and everything that names the contact follows in
//! one transaction. Every column that names a contact is classified, so a
//! new one cannot be missed.

use std::collections::HashSet;

use vauchi_core::contact::{Contact, Group};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::emergency::EmergencyBroadcastConfig;
use vauchi_core::storage::CONTACT_REFERENCES;
use vauchi_core::sync::guardian_piece::{GuardianDelivery, split_into_pieces};
use vauchi_core::{Identity, Storage, SymmetricKey};

fn contact(name: &str) -> Contact {
    Contact::from_exchange(
        *Identity::create(name, 0).signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

fn new_id() -> String {
    hex::encode(Identity::create("Alice, new phone", 0).signing_public_key())
}

/// Every column of either database that names a contact by its name or by
/// a foreign key to `contacts(id)`, as `(table, column, foreign_key)`.
fn columns_naming_a_contact(storage: &Storage) -> Vec<(String, String, bool)> {
    let mut found = Vec::new();
    for conn in [storage.connection(), storage.duress_connection()] {
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for table in tables {
            let foreign: Vec<String> = conn
                .prepare(&format!("PRAGMA foreign_key_list({table})"))
                .unwrap()
                .query_map([], |row| {
                    Ok((row.get::<_, String>(2)?, row.get::<_, String>(3)?))
                })
                .unwrap()
                .map(Result::unwrap)
                .filter(|(parent, _)| parent == "contacts")
                .map(|(_, from)| from)
                .collect();
            let columns: Vec<String> = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |row| row.get(1))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            for column in columns {
                let fk = foreign.contains(&column);
                if fk || column.contains("contact") {
                    found.push((table.clone(), column, fk));
                }
            }
        }
    }
    found
}

// @internal
#[test]
fn every_column_naming_a_contact_is_classified_for_a_move() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("v.db"), SymmetricKey::generate()).unwrap();

    let unclassified: Vec<(String, String)> = columns_naming_a_contact(&storage)
        .into_iter()
        .filter(|(table, column, fk)| {
            !fk && !CONTACT_REFERENCES
                .iter()
                .any(|(t, c, _)| t == table && c == column)
        })
        .map(|(table, column, _)| (table, column))
        .collect();

    assert!(unclassified.is_empty(), "{unclassified:?}");
}

// @internal
#[test]
fn a_moved_contact_keeps_its_notes_labels_tags_and_settings() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let alice = contact("Alice");
    let old = alice.id().to_string();
    let new = new_id();
    storage.contacts().save_contact(&alice).unwrap();
    storage
        .field_notes()
        .save_contact_field_note(&old, "phone", b"call after six")
        .unwrap();
    storage
        .labels()
        .save_group(&Group::from_storage(
            "family".into(),
            "Family".into(),
            HashSet::from([old.clone()]),
            HashSet::new(),
            None,
            None,
            None,
            0,
            0,
        ))
        .unwrap();
    let tag = storage.tags().create_tag("climbing").unwrap();
    storage.tags().add_to_tag(&tag.id, &old).unwrap();
    storage
        .labels()
        .save_contact_override(&old, "phone", true)
        .unwrap();
    storage
        .emergency()
        .save_emergency_config(&EmergencyBroadcastConfig {
            trusted_contact_ids: vec![old.clone()],
            message: "help".into(),
            include_location: false,
        })
        .unwrap();
    storage.recovery().set_guardian_group(&old, 1).unwrap();

    storage.move_contact_id(&old, &new).unwrap();

    assert!(storage.contacts().load_contact(&old).unwrap().is_none());
    assert_eq!(
        storage
            .contacts()
            .load_contact(&new)
            .unwrap()
            .unwrap()
            .display_name(),
        "Alice"
    );
    assert_eq!(
        storage
            .field_notes()
            .load_contact_field_notes(&new)
            .unwrap()
            .get("phone")
            .map(Vec::as_slice),
        Some(b"call after six".as_slice())
    );
    let family = storage.labels().load_all_groups().unwrap();
    assert_eq!(family[0].contacts(), &HashSet::from([new.clone()]));
    assert_eq!(
        storage
            .tags()
            .get_tag(&tag.id)
            .unwrap()
            .unwrap()
            .contact_ids,
        HashSet::from([new.clone()])
    );
    assert_eq!(
        storage
            .labels()
            .load_contact_overrides(&new)
            .unwrap()
            .get("phone"),
        Some(&true)
    );
    assert_eq!(
        storage
            .emergency()
            .load_emergency_config()
            .unwrap()
            .unwrap()
            .trusted_contact_ids,
        vec![new.clone()]
    );
    assert_eq!(
        storage.recovery().guardian_groups().unwrap(),
        vec![(1, vec![new.clone()])]
    );
}

// @internal
#[test]
fn a_backup_kept_for_the_moved_contact_is_kept_under_its_new_id() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let alice = contact("Alice");
    let old = alice.id().to_string();
    let new = new_id();
    storage.contacts().save_contact(&alice).unwrap();
    let delivery = GuardianDelivery::new(1, vec![7; 124], vec![1, 2, 3]).unwrap();
    for piece in split_into_pieces([3; 16], &delivery.encode()).unwrap() {
        storage
            .guardian_backups()
            .keep_piece(&old, &piece, 1)
            .unwrap();
    }

    storage.move_contact_id(&old, &new).unwrap();

    assert_eq!(
        storage.guardian_backups().kept(&new).unwrap(),
        Some(delivery)
    );
    assert_eq!(storage.guardian_backups().kept(&old).unwrap(), None);
}

// @internal
#[test]
fn a_move_onto_a_contact_that_exists_or_from_one_that_does_not_is_refused() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let alice = contact("Alice");
    let bob = contact("Bob");
    storage.contacts().save_contact(&alice).unwrap();
    storage.contacts().save_contact(&bob).unwrap();

    assert!(storage.move_contact_id(alice.id(), bob.id()).is_err());
    assert!(storage.move_contact_id(&new_id(), &new_id()).is_err());

    assert!(
        storage
            .contacts()
            .load_contact(alice.id())
            .unwrap()
            .is_some()
    );
    assert!(storage.contacts().load_contact(bob.id()).unwrap().is_some());
}

/// Alice's old contact with John's notes, label, tag, guardian group and a
/// kept backup, and the new contact an in-person exchange just made.
fn old_and_new(storage: &Storage) -> (String, Contact) {
    let old = contact("Alice");
    let old_id = old.id().to_string();
    storage.contacts().save_contact(&old).unwrap();
    storage
        .field_notes()
        .save_contact_field_note(&old_id, "phone", b"call after six")
        .unwrap();
    storage
        .field_notes()
        .save_contact_field_note(&old_id, "email", b"the old address")
        .unwrap();
    let tag = storage.tags().create_tag("climbing").unwrap();
    storage.tags().add_to_tag(&tag.id, &old_id).unwrap();
    storage.recovery().set_guardian_group(&old_id, 2).unwrap();
    storage
        .connection()
        .execute(
            "UPDATE contacts SET recovery_trusted = 1, favorite = 1 WHERE id = ?1",
            [&old_id],
        )
        .unwrap();
    let new = Contact::from_exchange(
        *Identity::create("Alice, new phone", 0).signing_public_key(),
        ContactCard::new("Alice (new phone)"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&new).unwrap();
    storage
        .field_notes()
        .save_contact_field_note(new.id(), "email", b"the new address")
        .unwrap();
    (old_id, new)
}

// @internal
#[test]
fn a_fold_keeps_the_new_contacts_keys_and_card_and_brings_what_only_the_old_one_held() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let (old_id, new) = old_and_new(&storage);

    storage.fold_contact_into(&old_id, new.id()).unwrap();

    assert!(storage.contacts().load_contact(&old_id).unwrap().is_none());
    let folded = storage.contacts().load_contact(new.id()).unwrap().unwrap();
    assert_eq!(folded.public_key(), new.public_key());
    assert_eq!(
        folded.shared_key().unwrap().as_bytes(),
        new.shared_key().unwrap().as_bytes()
    );
    assert_eq!(folded.display_name(), "Alice (new phone)");
    assert!(
        folded.is_recovery_trusted(),
        "the guardian role comes across"
    );
    let notes = storage
        .field_notes()
        .load_contact_field_notes(new.id())
        .unwrap();
    assert_eq!(
        notes.get("phone").map(Vec::as_slice),
        Some(b"call after six".as_slice())
    );
    assert_eq!(
        notes.get("email").map(Vec::as_slice),
        Some(b"the new address".as_slice()),
        "a row both hold keeps the new contact's"
    );
    let tags = storage.tags().list_tags().unwrap();
    assert_eq!(tags[0].contact_ids, HashSet::from([new.id().to_string()]));
    assert_eq!(
        storage.recovery().guardian_groups().unwrap(),
        vec![(2, vec![new.id().to_string()])]
    );
}

// @internal
#[test]
fn a_fold_needs_both_contacts() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let (old_id, new) = old_and_new(&storage);

    assert!(storage.fold_contact_into(&old_id, &new_id()).is_err());
    assert!(storage.fold_contact_into(&new_id(), new.id()).is_err());
    assert!(storage.contacts().load_contact(&old_id).unwrap().is_some());
}
