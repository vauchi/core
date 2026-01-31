// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for storage::consent

use vauchi_core::{Storage, SymmetricKey};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

#[test]
fn test_consent_upsert_and_check() {
    let storage = test_storage();
    storage
        .execute_consent_upsert("c1", "analytics", true, 1000)
        .unwrap();

    let granted = storage.check_consent("analytics").unwrap();
    assert!(granted);
}

#[test]
fn test_consent_check_not_granted() {
    let storage = test_storage();
    storage
        .execute_consent_upsert("c1", "analytics", false, 1000)
        .unwrap();

    let granted = storage.check_consent("analytics").unwrap();
    assert!(!granted);
}

#[test]
fn test_consent_check_nonexistent() {
    let storage = test_storage();
    let granted = storage.check_consent("nonexistent").unwrap();
    assert!(!granted);
}

#[test]
fn test_consent_latest_wins() {
    let storage = test_storage();
    storage
        .execute_consent_upsert("c1", "analytics", true, 1000)
        .unwrap();
    storage
        .execute_consent_upsert("c2", "analytics", false, 2000)
        .unwrap();

    let granted = storage.check_consent("analytics").unwrap();
    assert!(!granted); // Latest record says false
}

#[test]
fn test_list_consent_records() {
    let storage = test_storage();
    storage
        .execute_consent_upsert("c1", "analytics", true, 1000)
        .unwrap();
    storage
        .execute_consent_upsert("c2", "marketing", false, 2000)
        .unwrap();

    let records = storage.list_consent_records().unwrap();
    assert_eq!(records.len(), 2);

    // Sorted by timestamp
    assert_eq!(records[0].0, "c1");
    assert_eq!(records[0].1, "analytics");
    assert!(records[0].2); // granted
    assert_eq!(records[0].3, 1000);

    assert_eq!(records[1].0, "c2");
    assert_eq!(records[1].1, "marketing");
    assert!(!records[1].2); // not granted
    assert_eq!(records[1].3, 2000);
}

#[test]
fn test_list_consent_records_empty() {
    let storage = test_storage();
    let records = storage.list_consent_records().unwrap();
    assert!(records.is_empty());
}

#[test]
fn test_consent_upsert_overwrites() {
    let storage = test_storage();
    storage
        .execute_consent_upsert("c1", "analytics", true, 1000)
        .unwrap();
    // Same ID should overwrite
    storage
        .execute_consent_upsert("c1", "analytics", false, 2000)
        .unwrap();

    let records = storage.list_consent_records().unwrap();
    assert_eq!(records.len(), 1);
    assert!(!records[0].2);
}

#[test]
fn test_audit_log() {
    let storage = test_storage();
    storage
        .log_audit_event("data_export", Some("GDPR export triggered"))
        .unwrap();
    storage.log_audit_event("consent_change", None).unwrap();
    // Just confirm no errors — audit log is write-only from this interface
}
