// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Migration v70: the plaintext columns that v14/v15 emptied are dropped
//! (vauchi/private#535).

use rusqlite::{Connection, params};
use vauchi_core::crypto::{SymmetricKey, encrypt};
use vauchi_core::storage::Storage;
use vauchi_core::storage::migration::{MigrationRunner, all_migrations};
use vauchi_core::sync::VersionVector;
use vauchi_core::types::{AhaMomentTracker, AhaMomentType, DemoContactState};

const DROPPED_COLUMNS: &[(&str, &str)] = &[
    ("device_info", "device_name"),
    ("version_vector", "vector_json"),
    ("ux_state", "aha_tracker_json"),
    ("ux_state", "demo_contact_json"),
];

fn column_names(conn: &Connection, table: &str) -> Vec<String> {
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
fn migration_v70_drops_the_emptied_plaintext_columns() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    migrate_to(&conn, &key, 69);
    for (table, column) in DROPPED_COLUMNS {
        assert!(
            column_names(&conn, table).contains(&column.to_string()),
            "{table}.{column} should exist before v70"
        );
    }

    migrate_to(&conn, &key, 70);
    for (table, column) in DROPPED_COLUMNS {
        assert!(
            !column_names(&conn, table).contains(&column.to_string()),
            "{table}.{column} should be dropped by v70"
        );
    }
}

// @internal
#[test]
fn migration_v70_keeps_the_encrypted_rows_readable() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();

    let mut vector = VersionVector::new();
    vector.increment(&[0x07; 32]);
    let mut tracker = AhaMomentTracker::new();
    tracker.mark_seen(AhaMomentType::FirstEdit);
    let demo = DemoContactState::new_active(5);

    let device_id = [0x42u8; 32];
    {
        let conn = Connection::open(&db_path).unwrap();
        migrate_to(&conn, &key, 69);
        let device_json = serde_json::json!({
            "device_id": device_id.as_slice(),
            "device_index": 3,
            "device_name": "Old Phone",
            "created_at": 1234,
        });
        conn.execute(
            "INSERT INTO device_info (id, device_id, device_index, device_name, created_at, device_info_encrypted)
             VALUES (1, ?1, 3, '', 1234, ?2)",
            params![
                [0x42u8; 32].as_slice(),
                encrypt(&key, &serde_json::to_vec(&device_json).unwrap()).unwrap()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO version_vector (id, vector_json, vector_json_encrypted, updated_at)
             VALUES (1, '', ?1, 0)",
            params![encrypt(&key, vector.to_json().as_bytes()).unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ux_state (id, aha_tracker_json, aha_tracker_json_encrypted, demo_contact_json, demo_contact_json_encrypted, updated_at)
             VALUES (1, '', ?1, '', ?2, 0)",
            params![
                encrypt(&key, tracker.to_json().unwrap().as_bytes()).unwrap(),
                encrypt(&key, demo.to_json().unwrap().as_bytes()).unwrap()
            ],
        )
        .unwrap();
    }

    let storage = Storage::open(&db_path, key).unwrap();

    assert_eq!(
        storage.device().load_device_info().unwrap(),
        Some(([0x42; 32], 3, "Old Phone".to_string(), 1234))
    );
    let loaded_vector = storage.sync().load_version_vector().unwrap().unwrap();
    assert_eq!(loaded_vector.get(&[0x07; 32]), 1);
    let loaded_tracker = storage.ux().load_aha_tracker().unwrap().unwrap();
    assert!(loaded_tracker.has_seen(AhaMomentType::FirstEdit));
    assert!(!loaded_tracker.has_seen(AhaMomentType::CardCreationComplete));
    let loaded_demo = storage.ux().load_demo_contact_state().unwrap().unwrap();
    assert_eq!(loaded_demo.to_json().unwrap(), demo.to_json().unwrap());
}
