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
