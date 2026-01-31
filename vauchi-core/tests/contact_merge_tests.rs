// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact::merge (duplicate detection)

use vauchi_core::contact::merge::find_duplicates;
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, SymmetricKey};

fn make_contact(name: &str, fields: &[(FieldType, &str, &str)]) -> Contact {
    let pk = *vauchi_core::SigningKeyPair::generate()
        .public_key()
        .as_bytes();
    let mut card = ContactCard::new(name);
    for (ft, label, value) in fields {
        card.add_field(ContactField::new(ft.clone(), label, value))
            .unwrap();
    }
    Contact::from_exchange(pk, card, SymmetricKey::generate())
}

#[test]
fn test_no_duplicates_in_empty_list() {
    let contacts: Vec<Contact> = vec![];
    let dups = find_duplicates(&contacts);
    assert!(dups.is_empty());
}

#[test]
fn test_no_duplicates_single_contact() {
    let contacts = vec![make_contact("Alice", &[])];
    let dups = find_duplicates(&contacts);
    assert!(dups.is_empty());
}

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

#[test]
fn test_case_insensitive_name_match() {
    let contacts = vec![
        make_contact("alice smith", &[]),
        make_contact("Alice Smith", &[]),
    ];
    let dups = find_duplicates(&contacts);
    assert!(!dups.is_empty());
}

#[test]
fn test_different_names_no_duplicate() {
    let contacts = vec![make_contact("Alice", &[]), make_contact("Bob", &[])];
    let dups = find_duplicates(&contacts);
    assert!(dups.is_empty());
}

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

#[test]
fn test_partial_name_match() {
    // "Alice" is contained in "Alice Smith" → similarity ~0.8
    let contacts = vec![make_contact("Alice", &[]), make_contact("Alice Smith", &[])];
    let dups = find_duplicates(&contacts);
    // Partial containment gives 0.8 * 2 / 2 = 0.8 > 0.7 threshold
    assert!(!dups.is_empty());
}

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
    // First duplicate should have higher similarity
    assert!(dups[0].similarity >= dups[1].similarity);
}

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

#[test]
fn test_duplicate_pair_has_correct_ids() {
    let contacts = vec![make_contact("Alice", &[]), make_contact("Alice", &[])];
    let dups = find_duplicates(&contacts);
    assert!(!dups.is_empty());
    assert_eq!(dups[0].id1, contacts[0].id());
    assert_eq!(dups[0].id2, contacts[1].id());
}

#[test]
fn test_empty_name_contacts() {
    // Empty names won't match well with real names
    let contacts = vec![make_contact("", &[]), make_contact("Alice", &[])];
    let dups = find_duplicates(&contacts);
    assert!(dups.is_empty());
}
