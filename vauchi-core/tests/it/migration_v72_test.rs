// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Migration v72: visibility labels and contacts keep only their encrypted
//! columns (vauchi/private#535).

use rusqlite::{Connection, params};
use vauchi_core::crypto::{SymmetricKey, encrypt};
use vauchi_core::storage::Storage;
use vauchi_core::storage::migration::{MigrationRunner, all_migrations};

fn columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn migrate_to(conn: &Connection, key: &SymmetricKey, version: u32) {
    let subset: Vec<_> = all_migrations()
        .iter()
        .filter(|m| m.version <= version)
        .copied()
        .collect();
    MigrationRunner::run(conn, key, &subset, None, 0).unwrap();
}

// @internal
#[test]
fn migration_v72_leaves_labels_and_contacts_without_plaintext_columns() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();
    migrate_to(&conn, &key, 71);
    assert!(columns(&conn, "visibility_labels").contains(&"name".to_string()));
    assert!(columns(&conn, "contacts").contains(&"visibility_rules_json".to_string()));

    migrate_to(&conn, &key, 72);

    assert_eq!(
        columns(&conn, "visibility_labels"),
        [
            "id",
            "name_encrypted",
            "name_hmac",
            "contacts_json_encrypted",
            "visible_fields_json_encrypted",
            "display_name_override_encrypted",
            "bio_override_encrypted",
            "avatar_override_encrypted",
            "created_at",
            "modified_at",
        ]
    );
    assert!(!columns(&conn, "contacts").contains(&"visibility_rules_json".to_string()));
}

// @internal
#[test]
fn a_label_written_before_v72_loads_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    {
        let conn = Connection::open(&db_path).unwrap();
        migrate_to(&conn, &key, 71);
        conn.execute(
            "INSERT INTO visibility_labels
             (id, name, name_encrypted, contacts_json, visible_fields_json,
              contacts_json_encrypted, visible_fields_json_encrypted, created_at, modified_at)
             VALUES ('g1', 'g1', ?1, '[]', '[]', ?2, ?3, 10, 20)",
            params![
                encrypt(&key, b"Family").unwrap(),
                encrypt(&key, br#"["c1"]"#).unwrap(),
                encrypt(&key, br#"["email"]"#).unwrap()
            ],
        )
        .unwrap();
    }

    let storage = Storage::open(&db_path, key).unwrap();
    let label = storage.labels().load_group("g1").unwrap();

    assert_eq!(label.name(), "Family");
    assert_eq!(label.contacts().iter().collect::<Vec<_>>(), ["c1"]);
    assert_eq!(label.visible_fields().iter().collect::<Vec<_>>(), ["email"]);
    assert_eq!((label.created_at(), label.modified_at()), (10, 20));
}

// @internal
#[test]
fn a_label_without_its_encrypted_name_fails_to_load() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage
        .connection()
        .execute(
            "INSERT INTO visibility_labels (id, created_at, modified_at) VALUES ('g1', 1, 1)",
            [],
        )
        .unwrap();

    let err = storage.labels().load_group("g1").unwrap_err();

    assert!(
        err.to_string().contains("name_encrypted"),
        "unexpected error: {err}"
    );
}
