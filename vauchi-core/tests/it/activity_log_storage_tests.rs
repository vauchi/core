// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for storage::activity_log

use vauchi_core::{Storage, SymmetricKey, storage::ActivityLogRow};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn make_row(
    event_key: &str,
    category: &str,
    contact_id: Option<&str>,
    payload: &str,
    created_at: u64,
) -> ActivityLogRow {
    ActivityLogRow {
        event_key: event_key.to_string(),
        category: category.to_string(),
        contact_id: contact_id.map(str::to_string),
        payload: payload.to_string(),
        created_at,
    }
}

// @scenario: activity_log :: View recent activity
#[test]
fn insert_and_query_activity_log_entry() {
    let storage = test_storage();
    let now: u64 = 1_000_000;

    let row = make_row(
        "evt-001",
        "card_update",
        Some("contact-abc"),
        r#"{"field":"email"}"#,
        now,
    );
    let inserted = storage.activity_log().activity_log_insert(&row).unwrap();
    assert!(inserted, "first insert should return true");

    let results = storage
        .activity_log()
        .activity_log_query_recent(now, 3600)
        .unwrap();

    assert_eq!(results.len(), 1);
    let found = &results[0];
    assert_eq!(found.event_key, "evt-001");
    assert_eq!(found.category, "card_update");
    assert_eq!(found.contact_id, Some("contact-abc".to_string()));
    assert_eq!(found.payload, r#"{"field":"email"}"#);
    assert_eq!(found.created_at, now);
}

// @scenario: activity_log :: Duplicate events are deduplicated
#[test]
fn duplicate_event_key_is_ignored() {
    let storage = test_storage();
    let now: u64 = 1_000_000;

    let row = make_row("evt-dup", "exchange", None, r#"{}"#, now);

    let first = storage.activity_log().activity_log_insert(&row).unwrap();
    assert!(first, "first insert should return true");

    let second = storage.activity_log().activity_log_insert(&row).unwrap();
    assert!(
        !second,
        "second insert with same event_key should return false"
    );

    let results = storage
        .activity_log()
        .activity_log_query_recent(now, 3600)
        .unwrap();
    assert_eq!(
        results.len(),
        1,
        "only one entry should exist after duplicate insert"
    );
}

// @scenario: activity_log :: Old entries are pruned
#[test]
fn prune_removes_old_entries() {
    let storage = test_storage();

    // Anchor: "now" is day 8 from epoch (in seconds).
    let now: u64 = 8 * 24 * 3600; // 691_200
    let seven_days: u64 = 7 * 24 * 3600; // 604_800
    let one_day: u64 = 24 * 3600; // 86_400

    // Old entry: 8 days ago (before the 7-day window)
    let old_ts = now - 8 * 24 * 3600; // = 0, just at epoch
    let old_row = make_row("evt-old", "card_update", None, r#"{"old":true}"#, old_ts);
    storage
        .activity_log()
        .activity_log_insert(&old_row)
        .unwrap();

    // Recent entry: 1 day ago (within the 7-day window)
    let recent_ts = now - one_day;
    let recent_row = make_row("evt-recent", "exchange", None, r#"{"new":true}"#, recent_ts);
    storage
        .activity_log()
        .activity_log_insert(&recent_row)
        .unwrap();

    // Prune entries older than 7 days
    let deleted = storage
        .activity_log()
        .activity_log_prune(now, seven_days)
        .unwrap();
    assert_eq!(deleted, 1, "one old entry should be pruned");

    let results = storage
        .activity_log()
        .activity_log_query_recent(now, seven_days)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].event_key, "evt-recent");
}
