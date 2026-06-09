// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for dismissed duplicate storage operations (SP-12a).
//!
//! @scenario: contacts_management :: Dismiss duplicate suggestion

use crate::common;

// @scenario: contacts_management :: Dismiss duplicate suggestion
#[test]
fn test_dismiss_duplicate_persists() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let storage = wb.storage();

    storage
        .contacts()
        .dismiss_duplicate("id_a", "id_b")
        .unwrap();

    let dismissed = storage.contacts().load_dismissed_duplicates().unwrap();
    assert_eq!(dismissed.len(), 1);
    assert!(dismissed.contains(&("id_a".to_string(), "id_b".to_string())));
}

// @scenario: contacts_management :: Dismiss duplicate suggestion
#[test]
fn test_dismiss_duplicate_normalized_order() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let storage = wb.storage();

    storage
        .contacts()
        .dismiss_duplicate("id_z", "id_a")
        .unwrap();

    let dismissed = storage.contacts().load_dismissed_duplicates().unwrap();
    // Should be stored normalized (id_a, id_z)
    assert!(dismissed.contains(&("id_a".to_string(), "id_z".to_string())));
    assert!(!dismissed.contains(&("id_z".to_string(), "id_a".to_string())));
}

// @scenario: contacts_management :: Dismiss duplicate suggestion
#[test]
fn test_dismiss_duplicate_idempotent() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let storage = wb.storage();

    storage
        .contacts()
        .dismiss_duplicate("id_a", "id_b")
        .unwrap();
    storage
        .contacts()
        .dismiss_duplicate("id_a", "id_b")
        .unwrap(); // second call should not error

    let dismissed = storage.contacts().load_dismissed_duplicates().unwrap();
    assert_eq!(dismissed.len(), 1, "Duplicate dismiss should be idempotent");
}

// @scenario: contacts_management :: Dismiss duplicate suggestion
#[test]
fn test_dismiss_duplicate_reverse_also_idempotent() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let storage = wb.storage();

    storage
        .contacts()
        .dismiss_duplicate("id_a", "id_b")
        .unwrap();
    storage
        .contacts()
        .dismiss_duplicate("id_b", "id_a")
        .unwrap(); // reverse order

    let dismissed = storage.contacts().load_dismissed_duplicates().unwrap();
    assert_eq!(dismissed.len(), 1, "Reversed dismiss should be idempotent");
}

// @scenario: contacts_management :: Dismiss duplicate suggestion
#[test]
fn test_undismiss_duplicate_removes_entry() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let storage = wb.storage();

    storage
        .contacts()
        .dismiss_duplicate("id_a", "id_b")
        .unwrap();
    storage
        .contacts()
        .undismiss_duplicate("id_a", "id_b")
        .unwrap();

    let dismissed = storage.contacts().load_dismissed_duplicates().unwrap();
    assert!(
        dismissed.is_empty(),
        "Undismiss should remove the dismissed pair"
    );
}

// @scenario: contacts_management :: Dismiss duplicate suggestion
#[test]
fn test_load_dismissed_empty_initially() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let storage = wb.storage();

    let dismissed = storage.contacts().load_dismissed_duplicates().unwrap();
    assert!(dismissed.is_empty(), "No dismissals initially");
}

// @scenario: contacts_management :: Dismiss duplicate suggestion
#[test]
fn test_dismiss_multiple_pairs() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let storage = wb.storage();

    storage
        .contacts()
        .dismiss_duplicate("id_a", "id_b")
        .unwrap();
    storage
        .contacts()
        .dismiss_duplicate("id_c", "id_d")
        .unwrap();
    storage
        .contacts()
        .dismiss_duplicate("id_a", "id_c")
        .unwrap();

    let dismissed = storage.contacts().load_dismissed_duplicates().unwrap();
    assert_eq!(dismissed.len(), 3, "Should have 3 dismissed pairs");
}

// @scenario: contacts_management :: Dismiss duplicate suggestion
#[test]
fn test_dismiss_does_not_reappear_after_reload() {
    // This test verifies persistence across multiple load calls
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let storage = wb.storage();

    storage
        .contacts()
        .dismiss_duplicate("id_a", "id_b")
        .unwrap();

    let dismissed1 = storage.contacts().load_dismissed_duplicates().unwrap();
    let dismissed2 = storage.contacts().load_dismissed_duplicates().unwrap();
    assert_eq!(dismissed1, dismissed2, "Dismissed pairs should be stable");
}
