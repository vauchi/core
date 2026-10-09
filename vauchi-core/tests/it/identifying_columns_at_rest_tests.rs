// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identifying values are not stored in plaintext columns
//! (vauchi/private#579).

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn columns(storage: &Storage, table: &str) -> Vec<String> {
    let mut stmt = storage
        .connection()
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    stmt.query_map([], |row| row.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

// The name is already inside the encrypted identity blob.
// @internal
#[test]
fn the_identity_table_keeps_no_display_name() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    assert!(!columns(&storage, "identity").contains(&"display_name".to_string()));
}

// @internal
#[test]
fn a_decoy_contact_keeps_its_name_without_a_plaintext_column() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let card = vauchi_core::ContactCard::new("Decoy Card Name");

    storage
        .decoy()
        .save_decoy_contact("d1", "Decoy Alice", &card)
        .unwrap();
    let loaded = storage.decoy().load_decoy_contacts().unwrap();

    assert!(!columns(&storage, "decoy_contacts").contains(&"display_name".to_string()));
    assert_eq!(loaded.len(), 1);
    assert_eq!(
        (loaded[0].0.as_str(), loaded[0].1.as_str()),
        ("d1", "Decoy Alice")
    );
}

fn migrate_to(conn: &rusqlite::Connection, key: &SymmetricKey, version: u32) {
    use vauchi_core::storage::migration::{MigrationRunner, all_migrations};
    let subset: Vec<_> = all_migrations()
        .iter()
        .filter(|m| m.version <= version)
        .copied()
        .collect();
    MigrationRunner::run(conn, key, &subset, None, 0).unwrap();
}

// @internal
#[test]
fn a_decoy_name_stored_before_v77_survives_encryption() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    let card = vauchi_core::ContactCard::new("Card");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        migrate_to(&conn, &key, 76);
        let card_enc =
            vauchi_core::crypto::encrypt(&key, &serde_json::to_vec(&card).unwrap()).unwrap();
        conn.execute(
            "INSERT INTO decoy_contacts (id, display_name, card_encrypted, created_at, updated_at)
             VALUES ('d1', 'Old Decoy', ?1, 1, 1)",
            [card_enc],
        )
        .unwrap();
    }

    let storage = Storage::open(&db_path, key).unwrap();
    let loaded = storage.decoy().load_decoy_contacts().unwrap();

    assert_eq!(loaded[0].1, "Old Decoy");
}

fn saved_contact(storage: &Storage) -> String {
    let contact = vauchi_core::contact::Contact::from_exchange(
        [7u8; 32],
        vauchi_core::ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();
    contact.id().to_string()
}

// @internal
#[test]
fn the_name_last_sent_to_a_contact_round_trips_without_a_plaintext_column() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let id = saved_contact(&storage);

    assert_eq!(
        storage.contacts().load_last_sent_display_name(&id).unwrap(),
        None
    );
    storage
        .contacts()
        .save_last_sent_display_name(&id, "Ada Lovelace")
        .unwrap();

    assert_eq!(
        storage
            .contacts()
            .load_last_sent_display_name(&id)
            .unwrap()
            .as_deref(),
        Some("Ada Lovelace")
    );
    assert!(!columns(&storage, "contacts").contains(&"last_sent_display_name".to_string()));
}

// @internal
#[test]
fn a_last_sent_name_stored_before_v78_survives_encryption() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        migrate_to(&conn, &key, 77);
        conn.execute(
            "INSERT INTO contacts (id, public_key, card_encrypted, shared_key_encrypted,
                                   exchange_timestamp, last_sent_display_name)
             VALUES ('c1', X'07', X'00', X'00', 0, 'Ada')",
            [],
        )
        .unwrap();
    }

    let storage = Storage::open(&db_path, key).unwrap();

    assert_eq!(
        storage
            .contacts()
            .load_last_sent_display_name("c1")
            .unwrap()
            .as_deref(),
        Some("Ada")
    );
}
