// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A guardian is never told it is one until the owner needs it (#459), and
//! its database does not say for whom either (vauchi/private#796, D15): the
//! rows of what it keeps carry a keyed tag of the designator, never the
//! designator's contact id, also after a storage rekey.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::sync::guardian_piece::{GuardianDelivery, split_into_pieces};
use vauchi_core::{Storage, SymmetricKey};

fn storage_with_designator() -> (Storage, String) {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let pk = *SigningKeyPair::generate().public_key().as_bytes();
    let contact =
        Contact::from_exchange(pk, ContactCard::new("Alice"), SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    (storage, id)
}

/// Every value in `table` that equals `needle`, over every column.
fn mentions(storage: &Storage, table: &str, needle: &str) -> usize {
    let conn = storage.connection();
    let columns: Vec<String> = conn
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    columns
        .iter()
        .map(|column| {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE CAST({column} AS TEXT) = ?1"),
                [needle],
                |row| row.get::<_, i64>(0),
            )
            .unwrap() as usize
        })
        .sum()
}

fn keep_one_whole_and_one_waiting(storage: &Storage, designator: &str) {
    let whole = GuardianDelivery::new(1, vec![1; 124], vec![1; 60_000]).unwrap();
    for piece in split_into_pieces([1; 16], &whole.encode()).unwrap() {
        storage
            .guardian_backups()
            .keep_piece(designator, &piece, 1)
            .unwrap();
    }
    let waiting = GuardianDelivery::new(2, vec![2; 124], vec![2; 60_000]).unwrap();
    let first = split_into_pieces([2; 16], &waiting.encode())
        .unwrap()
        .remove(0);
    storage
        .guardian_backups()
        .keep_piece(designator, &first, 2)
        .unwrap();
}

// @internal
#[test]
fn the_guardian_tables_never_name_the_designator() {
    let (storage, alice) = storage_with_designator();

    keep_one_whole_and_one_waiting(&storage, &alice);

    assert!(storage.guardian_backups().kept(&alice).unwrap().is_some());
    assert_eq!(
        storage
            .guardian_backups()
            .unfinished_backups(&alice)
            .unwrap(),
        1
    );
    assert_eq!(mentions(&storage, "guardian_backups", &alice), 0);
    assert_eq!(mentions(&storage, "guardian_backup_pieces", &alice), 0);
}

// @internal
#[test]
fn after_a_rekey_the_backup_is_found_and_the_designator_still_not_named() {
    let (mut storage, alice) = storage_with_designator();
    keep_one_whole_and_one_waiting(&storage, &alice);

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage
            .guardian_backups()
            .kept(&alice)
            .unwrap()
            .map(|delivery| delivery.created_at()),
        Some(1)
    );
    assert_eq!(
        storage
            .guardian_backups()
            .unfinished_backups(&alice)
            .unwrap(),
        1
    );
    assert_eq!(mentions(&storage, "guardian_backups", &alice), 0);
    assert_eq!(mentions(&storage, "guardian_backup_pieces", &alice), 0);
}
