// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact merge and dismiss duplicate operations (SP-12a).
//!
//! @scenario: contacts_management :: Merge duplicate contacts
//! @scenario: contacts_management :: Dismiss duplicate suggestion

use std::collections::HashSet;

use vauchi_core::contact::merge::{
    filter_dismissed, find_duplicates, merge_contacts, normalize_pair_key,
};
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, SymmetricKey};

fn make_contact(name: &str, fields: &[(FieldType, &str, &str)]) -> Contact {
    let pk = *vauchi_core::SigningKeyPair::generate()
        .public_key()
        .as_bytes();
    let mut card = ContactCard::new(name);
    for (ft, label, value) in fields {
        card.add_field(ContactField::new(ft.clone(), label, value, 0))
            .unwrap();
    }
    Contact::from_exchange(pk, card, SymmetricKey::generate(), 0)
}

// ============================================================
// @scenario: contacts_management :: Merge duplicate contacts
// ============================================================

// @internal
#[test]
fn test_merge_contacts_preserves_primary_display_name() {
    let primary = make_contact("Bob Smith", &[]);
    let secondary = make_contact("Robert Smith", &[]);

    let merged = merge_contacts(&primary, &secondary);
    assert_eq!(
        merged.display_name(),
        "Bob Smith",
        "Primary display name should be preserved"
    );
}

// @internal
#[test]
fn test_merge_contacts_preserves_primary_id() {
    let primary = make_contact("Bob Smith", &[]);
    let secondary = make_contact("Robert Smith", &[]);

    let merged = merge_contacts(&primary, &secondary);
    assert_eq!(
        merged.id(),
        primary.id(),
        "Primary contact ID should be preserved"
    );
}

// @internal
#[test]
fn test_merge_contacts_adds_unique_fields_from_secondary() {
    let primary = make_contact("Bob Smith", &[(FieldType::Email, "work", "bob@work.com")]);
    let secondary = make_contact(
        "Robert Smith",
        &[(FieldType::Phone, "mobile", "+1-555-123-4567")],
    );

    let merged = merge_contacts(&primary, &secondary);
    let fields = merged.card().fields();
    assert_eq!(fields.len(), 2, "Merged contact should have both fields");

    let has_email = fields.iter().any(|f| f.field_type() == FieldType::Email);
    let has_phone = fields.iter().any(|f| f.field_type() == FieldType::Phone);
    assert!(has_email, "Should have email from primary");
    assert!(has_phone, "Should have phone from secondary");
}

// @internal
#[test]
fn test_merge_contacts_does_not_duplicate_same_field() {
    let primary = make_contact("Bob Smith", &[(FieldType::Email, "work", "bob@work.com")]);
    let secondary = make_contact(
        "Robert Smith",
        &[(FieldType::Email, "work", "robert@work.com")],
    );

    let merged = merge_contacts(&primary, &secondary);
    let fields = merged.card().fields();
    // Same type + label, so secondary's field should not be added
    assert_eq!(
        fields.len(),
        1,
        "Should not duplicate fields with same type and label"
    );
    assert_eq!(
        fields[0].value(),
        "bob@work.com",
        "Primary's field value should be kept"
    );
}

// @internal
#[test]
fn test_merge_contacts_different_labels_both_kept() {
    let primary = make_contact("Bob Smith", &[(FieldType::Email, "work", "bob@work.com")]);
    let secondary = make_contact(
        "Robert Smith",
        &[(FieldType::Email, "personal", "bob@home.com")],
    );

    let merged = merge_contacts(&primary, &secondary);
    let fields = merged.card().fields();
    assert_eq!(
        fields.len(),
        2,
        "Different labels should result in both fields being kept"
    );
}

// @internal
#[test]
fn test_merge_contacts_preserves_all_info() {
    let primary = make_contact(
        "Bob Smith",
        &[
            (FieldType::Email, "work", "bob@work.com"),
            (FieldType::Phone, "home", "+1-555-000-1111"),
        ],
    );
    let secondary = make_contact(
        "Robert Smith",
        &[
            (FieldType::Phone, "mobile", "+1-555-222-3333"),
            (FieldType::Address, "office", "123 Main St"),
        ],
    );

    let merged = merge_contacts(&primary, &secondary);
    let fields = merged.card().fields();
    // Primary: email(work), phone(home)
    // Secondary adds: phone(mobile), address(office)
    assert_eq!(
        fields.len(),
        4,
        "All unique fields should be preserved: email(work), phone(home), phone(mobile), address(office)"
    );
}

// @internal
#[test]
fn test_merge_contacts_preserves_favorite_from_primary() {
    let mut primary = make_contact("Bob Smith", &[]);
    primary.set_favorite(true);
    let secondary = make_contact("Robert Smith", &[]);

    let merged = merge_contacts(&primary, &secondary);
    assert!(
        merged.is_favorite(),
        "Primary's favorite status should be preserved"
    );
}

// @internal
#[test]
fn test_merge_contacts_preserves_blocked_from_primary() {
    let mut primary = make_contact("Bob Smith", &[]);
    primary.block();
    let secondary = make_contact("Robert Smith", &[]);

    let merged = merge_contacts(&primary, &secondary);
    assert!(
        merged.is_blocked(),
        "Primary's blocked status should be preserved"
    );
}

// ============================================================
// @scenario: contacts_management :: Dismiss duplicate suggestion
// ============================================================

// @internal
#[test]
fn test_normalize_pair_key_sorts_lexicographically() {
    let (a, b) = normalize_pair_key("zzz", "aaa");
    assert_eq!(a, "aaa");
    assert_eq!(b, "zzz");
}

// @internal
#[test]
fn test_normalize_pair_key_already_sorted() {
    let (a, b) = normalize_pair_key("aaa", "zzz");
    assert_eq!(a, "aaa");
    assert_eq!(b, "zzz");
}

// @internal
#[test]
fn test_normalize_pair_key_equal_ids() {
    let (a, b) = normalize_pair_key("same", "same");
    assert_eq!(a, "same");
    assert_eq!(b, "same");
}

// @internal
#[test]
fn test_filter_dismissed_removes_dismissed_pairs() {
    let c1 = make_contact("Alice", &[]);
    let c2 = make_contact("Alice", &[]);
    let c3 = make_contact("Alice", &[]);
    let contacts = vec![c1.clone(), c2.clone(), c3.clone()];

    let duplicates = find_duplicates(&contacts);
    assert!(duplicates.len() >= 2, "Should detect multiple pairs");

    let mut dismissed = HashSet::new();
    let key = normalize_pair_key(&duplicates[0].id1, &duplicates[0].id2);
    dismissed.insert(key.clone());

    let filtered = filter_dismissed(duplicates.clone(), &dismissed);
    assert_eq!(
        filtered.len(),
        duplicates.len() - 1,
        "Should have one fewer pair after dismissal"
    );

    for pair in &filtered {
        let pair_key = normalize_pair_key(&pair.id1, &pair.id2);
        assert_ne!(
            pair_key, key,
            "Dismissed pair should not appear in filtered results"
        );
    }
}

// @internal
#[test]
fn test_filter_dismissed_keeps_non_dismissed() {
    let c1 = make_contact("Alice", &[]);
    let c2 = make_contact("Alice", &[]);
    let contacts = vec![c1, c2];

    let duplicates = find_duplicates(&contacts);
    assert_eq!(duplicates.len(), 1);

    let dismissed: HashSet<(String, String)> = HashSet::new();
    let filtered = filter_dismissed(duplicates, &dismissed);
    assert_eq!(filtered.len(), 1, "No dismissals, all pairs should remain");
}

// @internal
#[test]
fn test_filter_dismissed_all_dismissed() {
    let c1 = make_contact("Alice", &[]);
    let c2 = make_contact("Alice", &[]);
    let contacts = vec![c1, c2];

    let duplicates = find_duplicates(&contacts);
    assert_eq!(duplicates.len(), 1);

    let mut dismissed = HashSet::new();
    let key = normalize_pair_key(&duplicates[0].id1, &duplicates[0].id2);
    dismissed.insert(key);

    let filtered = filter_dismissed(duplicates, &dismissed);
    assert!(
        filtered.is_empty(),
        "All pairs dismissed, result should be empty"
    );
}

// @internal
#[test]
fn test_filter_dismissed_order_independent() {
    // Dismissing (B, A) should also filter out (A, B) due to normalization
    let c1 = make_contact("Alice", &[]);
    let c2 = make_contact("Alice", &[]);
    let contacts = vec![c1, c2];

    let duplicates = find_duplicates(&contacts);
    assert_eq!(duplicates.len(), 1);

    let mut dismissed = HashSet::new();
    let key = normalize_pair_key(&duplicates[0].id2, &duplicates[0].id1);
    dismissed.insert(key);

    let filtered = filter_dismissed(duplicates, &dismissed);
    assert!(
        filtered.is_empty(),
        "Reversed pair should still be recognized as dismissed"
    );
}
