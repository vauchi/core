// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device that ran migrations v105 and v106 as Core 0.98.0 shipped them
//! upgrades in place (vauchi/private#796): v108 adds the answers' `kept_at`
//! and drops the kept-offer table D18 no longer uses, and a rerun changes
//! nothing.

use rusqlite::Connection;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::migration::{MigrationRunner, all_migrations};

fn run_up_to(conn: &Connection, key: &SymmetricKey, up_to: u32) {
    let subset: Vec<_> = all_migrations()
        .iter()
        .filter(|migration| migration.version <= up_to)
        .copied()
        .collect();
    MigrationRunner::run(conn, key, &subset, None, 0).unwrap();
}

fn columns(conn: &Connection, table: &str) -> Vec<String> {
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn has_table(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

// @internal
#[test]
fn a_device_on_the_shipped_v106_upgrades_its_answers_and_drops_the_kept_offers() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();
    run_up_to(&conn, &key, 107);
    assert!(!columns(&conn, "recovery_rekey_answers").contains(&"kept_at".to_string()));
    assert!(has_table(&conn, "recovery_rekey_inbound"));
    // The answer's contact is beside the point here.
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    conn.execute(
        "INSERT INTO recovery_rekey_answers (contact_id, answer_encrypted, old_shared_encrypted) \
         VALUES ('c1', x'01', x'02')",
        [],
    )
    .unwrap();

    run_up_to(&conn, &key, u32::MAX);

    assert!(columns(&conn, "recovery_rekey_answers").contains(&"kept_at".to_string()));
    assert!(!has_table(&conn, "recovery_rekey_inbound"));
    let kept_at: i64 = conn
        .query_row(
            "SELECT kept_at FROM recovery_rekey_answers WHERE contact_id = 'c1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept_at, 0, "an answer kept before v108 has no time yet");
}

// @internal
#[test]
fn v108_rerun_over_the_current_schema_changes_nothing() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();
    run_up_to(&conn, &key, u32::MAX);
    let before = columns(&conn, "recovery_rekey_answers");

    let v108: Vec<_> = all_migrations()
        .iter()
        .filter(|migration| migration.version == 108)
        .copied()
        .collect();
    assert_eq!(v108.len(), 1);
    MigrationRunner::run(&conn, &key, &v108, None, 0).unwrap();

    assert_eq!(columns(&conn, "recovery_rekey_answers"), before);
}
