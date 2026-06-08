// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact::merge (duplicate detection)

use vauchi_core::contact::merge::{compute_similarity, find_duplicates, normalize_phone};
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, ImportSource, SymmetricKey};

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

fn make_imported_contact(name: &str, fields: &[(FieldType, &str, &str)]) -> Contact {
    let mut card = ContactCard::new(name);
    for (ft, label, value) in fields {
        card.add_field(ContactField::new(ft.clone(), label, value, 0))
            .unwrap();
    }
    Contact::from_import(card, ImportSource::VcardFile, None, 0)
}

// @internal
#[test]
fn test_no_duplicates_in_empty_list() {
    let contacts: Vec<Contact> = vec![];
    let dups = find_duplicates(&contacts);
    assert!(dups.is_empty());
}

// @internal
#[test]
fn test_no_duplicates_single_contact() {
    let contacts = vec![make_contact("Alice", &[])];
    let dups = find_duplicates(&contacts);
    assert!(dups.is_empty());
}

// @scenario: contacts_management :: Detect potential duplicate contacts
// @internal
#[test]
fn test_exact_name_duplicate() {
    let contacts = vec![
        make_contact("Alice Smith", &[]),
        make_contact("Alice Smith", &[]),
    ];
    let dups = find_duplicates(&contacts);
    assert!(!dups.is_empty());
    assert!(dups[0].similarity >= 0.7);
}

// @scenario: contacts_management :: Detect potential duplicate contacts
// @internal
#[test]
fn test_case_insensitive_name_match() {
    let contacts = vec![
        make_contact("alice smith", &[]),
        make_contact("Alice Smith", &[]),
    ];
    let dups = find_duplicates(&contacts);
    assert!(!dups.is_empty());
}

// @scenario: contacts_management :: Detect potential duplicate contacts
// @internal
#[test]
fn test_different_names_no_duplicate() {
    let contacts = vec![make_contact("Alice", &[]), make_contact("Bob", &[])];
    let dups = find_duplicates(&contacts);
    assert!(dups.is_empty());
}

// @scenario: contacts_management :: Detect potential duplicate contacts
// @internal
#[test]
fn test_similar_names_with_shared_fields() {
    let contacts = vec![
        make_contact(
            "Alice Smith",
            &[(FieldType::Email, "work", "alice@example.com")],
        ),
        make_contact(
            "Alice Smith",
            &[(FieldType::Email, "work", "alice@example.com")],
        ),
    ];
    let dups = find_duplicates(&contacts);
    assert!(!dups.is_empty());
    assert!(dups[0].similarity >= 0.9);
}

// @scenario: contacts_management :: Detect potential duplicate contacts
// @internal
#[test]
fn test_partial_name_match() {
    // "Alice" is contained in "Alice Smith" → similarity ~0.8
    let contacts = vec![make_contact("Alice", &[]), make_contact("Alice Smith", &[])];
    let dups = find_duplicates(&contacts);
    // Partial containment gives 0.8 * 2 / 2 = 0.8 > 0.7 threshold
    assert!(!dups.is_empty());
}

// @scenario: contacts_management :: Detect potential duplicate contacts
// @internal
#[test]
fn test_duplicates_sorted_by_similarity() {
    let contacts = vec![
        make_contact(
            "Alice Smith",
            &[(FieldType::Email, "work", "alice@example.com")],
        ),
        make_contact(
            "Alice Smith",
            &[(FieldType::Email, "work", "alice@example.com")],
        ),
        make_contact("Alice Smith", &[]),
    ];
    let dups = find_duplicates(&contacts);
    assert!(dups.len() >= 2);
    assert!(dups[0].similarity >= dups[1].similarity);
}

// @scenario: contacts_management :: Detect potential duplicate contacts
// @internal
#[test]
fn test_three_contacts_detects_all_pairs() {
    let contacts = vec![
        make_contact("Alice", &[]),
        make_contact("Alice", &[]),
        make_contact("Alice", &[]),
    ];
    let dups = find_duplicates(&contacts);
    // 3 contacts → 3 pairs
    assert_eq!(dups.len(), 3);
}

// @scenario: contacts_management :: Detect potential duplicate contacts
// @internal
#[test]
fn test_duplicate_pair_has_correct_ids() {
    let contacts = vec![make_contact("Alice", &[]), make_contact("Alice", &[])];
    let dups = find_duplicates(&contacts);
    assert!(!dups.is_empty());
    assert_eq!(dups[0].id1, contacts[0].id());
    assert_eq!(dups[0].id2, contacts[1].id());
}

// @internal
#[test]
fn test_empty_name_contacts() {
    // Empty names won't match well with real names
    let contacts = vec![make_contact("", &[]), make_contact("Alice", &[])];
    let dups = find_duplicates(&contacts);
    assert!(dups.is_empty());
}

// @internal
#[test]
fn test_duplicate_detection_nfc_vs_nfd() {
    // NFC (precomposed) and NFD (decomposed) versions of "José" should
    // produce identical display names after normalization, so duplicate
    // detection should find them as exact matches.
    let nfc_contact = make_contact("Jos\u{00E9}", &[]); // NFC: é
    let nfd_contact = make_contact("Jose\u{0301}", &[]); // NFD: e + combining acute

    assert_eq!(
        nfc_contact.card().display_name(),
        nfd_contact.card().display_name(),
        "NFC and NFD names should be identical after normalization"
    );
}

// ── Phone normalization (Task 8: Dedup extension) ─────────────────────────

// @internal
#[test]
fn phone_normalization_strips_formatting() {
    assert_eq!(normalize_phone("+1 (555) 123-4567"), "15551234567");
    assert_eq!(normalize_phone("+1-555-123-4567"), "15551234567");
    assert_eq!(normalize_phone("15551234567"), "15551234567");
    assert_eq!(normalize_phone("+49 170 1234567"), "491701234567");
    assert_eq!(normalize_phone(""), "");
}

// @internal
#[test]
fn phone_normalization_retains_only_digits() {
    // Letters, spaces, parens, dashes, plus all stripped
    assert_eq!(normalize_phone("(800) FLOWERS"), "800");
    assert_eq!(normalize_phone("tel:+1-800-555-1234"), "18005551234");
}

// @scenario: contacts_management :: Detect duplicate contacts across import sources
// @internal
#[test]
fn cross_kind_dedup_finds_phone_match() {
    // An exchanged contact and an imported contact with the same phone number
    // but different formatting should exceed the dedup threshold.
    let exchanged = make_contact(
        "Alice Smith",
        &[(FieldType::Phone, "mobile", "+1 (555) 123-4567")],
    );
    let imported = make_imported_contact(
        "Alice Smith",
        &[(FieldType::Phone, "mobile", "15551234567")],
    );

    let sim = compute_similarity(&exchanged, &imported);
    assert!(
        sim >= 0.7,
        "same phone in different formats must produce similarity >= 0.7, got {}",
        sim
    );
}

// @internal
#[test]
fn different_phones_stay_below_threshold() {
    // from phone alone (need name match too).
    let a = make_contact(
        "Alice Smith",
        &[(FieldType::Phone, "mobile", "+15551110000")],
    );
    let b = make_contact("Bob Jones", &[(FieldType::Phone, "mobile", "+15559990000")]);

    let sim = compute_similarity(&a, &b);
    assert!(
        sim < 0.7,
        "different names + different phones must be below threshold, got {}",
        sim
    );
}

// @scenario: contact_management :: Preview merge shows unique fields
// @internal
#[test]
fn test_preview_merge_additions() {
    use vauchi_core::contact::merge::preview_merge_additions;

    let primary = make_contact(
        "Alice",
        &[
            (FieldType::Phone, "Mobile", "+1-555-1234"),
            (FieldType::Email, "Work", "alice@work.com"),
        ],
    );
    let secondary = make_contact(
        "Alice A.",
        &[
            (FieldType::Phone, "Mobile", "+1-555-1234"), // duplicate
            (FieldType::Email, "Personal", "alice@home.com"), // unique (diff label)
            (FieldType::Website, "Blog", "https://alice.dev"), // unique (new type)
        ],
    );

    let additions = preview_merge_additions(&primary, &secondary);
    assert_eq!(additions.len(), 2, "Should find 2 unique fields");
    let labels: Vec<&str> = additions.iter().map(|f| f.label()).collect();
    assert!(labels.contains(&"Personal"));
    assert!(labels.contains(&"Blog"));
}
