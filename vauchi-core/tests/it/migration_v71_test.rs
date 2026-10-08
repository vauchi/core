// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Migration v71: `device_info` keeps only its encrypted blob; the
//! plaintext device id, index and creation time go (vauchi/private#535).

use rusqlite::Connection;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::storage::migration::{MigrationRunner, all_migrations};

fn device_info_columns(conn: &Connection) -> Vec<String> {
    let mut stmt = conn.prepare("PRAGMA table_info(device_info)").unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

// @internal
#[test]
fn migration_v71_leaves_device_info_with_only_its_encrypted_blob() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();
    let up_to_v70: Vec<_> = all_migrations()
        .iter()
        .filter(|m| m.version <= 70)
        .copied()
        .collect();
    MigrationRunner::run(&conn, &key, &up_to_v70, None, 0).unwrap();
    assert_eq!(
        device_info_columns(&conn),
        [
            "id",
            "device_id",
            "device_index",
            "created_at",
            "device_info_encrypted"
        ]
    );

    MigrationRunner::run(&conn, &key, all_migrations(), None, 0).unwrap();

    assert_eq!(device_info_columns(&conn), ["id", "device_info_encrypted"]);
}

// @internal
#[test]
fn device_info_round_trips_after_v71() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage
        .device()
        .save_device_info(&[0x24; 32], 2, "Tablet", 777)
        .unwrap();

    assert_eq!(
        storage.device().load_device_info().unwrap(),
        Some(([0x24; 32], 2, "Tablet".to_string(), 777))
    );
}
