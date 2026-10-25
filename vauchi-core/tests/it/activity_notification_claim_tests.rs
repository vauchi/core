// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::storage::migration::{MigrationRunner, all_migrations};
use vauchi_core::{Storage, SymmetricKey, storage::ActivityLogRow};

fn key() -> SymmetricKey {
    SymmetricKey::from_bytes([7; 32])
}

fn row(event_key: &str, created_at: u64) -> ActivityLogRow {
    ActivityLogRow {
        event_key: event_key.into(),
        category: "emergency".into(),
        contact_id: Some("bob".into()),
        payload: "{}".into(),
        created_at,
    }
}

// @internal — handoff claims persist independently of event timestamps (#764).
#[test]
fn notification_claims_survive_reopening_and_do_not_consume_pending_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("activity.db");
    let first = row("first", 100);
    let pending = row("pending", 10);
    {
        let storage = Storage::open(&path, key()).unwrap();
        storage.activity_log().activity_log_insert(&first).unwrap();
        assert_eq!(
            storage.activity_log().claim_notifications().unwrap(),
            vec![first.clone()]
        );
        storage
            .activity_log()
            .activity_log_insert(&pending)
            .unwrap();
    }
    let storage = Storage::open(&path, key()).unwrap();
    let other = Storage::open(&path, key()).unwrap();
    assert_eq!(
        storage.activity_log().claim_notifications().unwrap(),
        vec![pending]
    );
    assert_eq!(
        other.activity_log().claim_notifications().unwrap(),
        Vec::new()
    );
    assert_eq!(
        storage
            .activity_log()
            .activity_log_query_recent(100, 100)
            .unwrap()
            .len(),
        2
    );
}

// @internal — upgrading cannot replay historical rows with unknown delivery state (#764).
#[test]
fn notification_claim_migration_preserves_history_without_replaying_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.db");
    let old = row("legacy", 100);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let migrations: Vec<_> = all_migrations()
            .iter()
            .copied()
            .filter(|m| m.version <= 85)
            .collect();
        MigrationRunner::run(&conn, &key(), &migrations, None, 100).unwrap();
        conn.execute("INSERT INTO activity_log (event_key, category, contact_id, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![old.event_key, old.category, old.contact_id, old.payload, old.created_at]).unwrap();
    }
    let storage = Storage::open(&path, key()).unwrap();
    assert_eq!(
        storage
            .activity_log()
            .activity_log_query_recent(100, 10)
            .unwrap(),
        vec![old]
    );
    assert_eq!(
        storage.activity_log().claim_notifications().unwrap(),
        Vec::new()
    );
    let new = row("new", 100);
    storage.activity_log().activity_log_insert(&new).unwrap();
    assert_eq!(
        storage.activity_log().claim_notifications().unwrap(),
        vec![new]
    );
}

// @internal — pruning and duplicate inserts cannot reset another row's claim (#764).
#[test]
fn notification_claims_handle_duplicates_and_an_empty_pruned_table() {
    let storage = Storage::in_memory(key()).unwrap();
    let first = row("first", 1);
    storage.activity_log().activity_log_insert(&first).unwrap();
    assert_eq!(
        storage.activity_log().claim_notifications().unwrap(),
        vec![first.clone()]
    );
    assert_eq!(
        storage.activity_log().activity_log_insert(&first).unwrap(),
        false
    );
    assert_eq!(
        storage.activity_log().claim_notifications().unwrap(),
        Vec::new()
    );
    assert_eq!(
        storage.activity_log().activity_log_prune(100, 1).unwrap(),
        1
    );
    let next = row("next", 100);
    storage.activity_log().activity_log_insert(&next).unwrap();
    assert_eq!(
        storage.activity_log().claim_notifications().unwrap(),
        vec![next]
    );
}

// @internal — a failed claim must leave every row available for retry (#764).
#[test]
fn notification_claim_failure_is_atomic() {
    let storage = Storage::in_memory(key()).unwrap();
    for entry in [row("a", 1), row("b", 2)] {
        storage.activity_log().activity_log_insert(&entry).unwrap();
    }
    storage.connection().execute_batch("CREATE TRIGGER reject_claim BEFORE UPDATE ON activity_log WHEN NEW.event_key = 'b' BEGIN SELECT RAISE(ABORT, 'injected claim failure'); END;").unwrap();
    let error = storage.activity_log().claim_notifications().unwrap_err();
    assert!(
        error.to_string().contains("injected claim failure"),
        "{error}"
    );
    storage
        .connection()
        .execute_batch("DROP TRIGGER reject_claim;")
        .unwrap();
    let mut claimed = storage.activity_log().claim_notifications().unwrap();
    claimed.sort_by(|a, b| a.event_key.cmp(&b.event_key));
    assert_eq!(claimed, vec![row("a", 1), row("b", 2)]);
}

// @internal — decoding a malformed database row must roll back every handoff (#764).
#[test]
fn notification_claim_decode_failure_keeps_valid_rows_pending() {
    let storage = Storage::in_memory(key()).unwrap();
    for entry in [row("a", 1), row("b", 2)] {
        storage.activity_log().activity_log_insert(&entry).unwrap();
    }
    storage
        .connection()
        .execute(
            "UPDATE activity_log SET payload = x'80' WHERE event_key = 'b'",
            [],
        )
        .unwrap();
    let error = storage.activity_log().claim_notifications().unwrap_err();
    assert!(
        error.to_string().contains("Invalid column type Blob"),
        "{error}"
    );
    storage
        .connection()
        .execute(
            "UPDATE activity_log SET payload = '{}' WHERE event_key = 'b'",
            [],
        )
        .unwrap();
    let mut claimed = storage.activity_log().claim_notifications().unwrap();
    claimed.sort_by(|a, b| a.event_key.cmp(&b.event_key));
    assert_eq!(claimed, vec![row("a", 1), row("b", 2)]);
}
