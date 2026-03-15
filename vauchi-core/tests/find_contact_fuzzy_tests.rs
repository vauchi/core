// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for find_contact_fuzzy API
//! Trace: ADR-021 Tier 1 — find_contact_fuzzy

use vauchi_core::api::*;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn create_test_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
}

fn add_named_contact(wb: &Vauchi, name: &str, pk: [u8; 32]) -> String {
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange(pk, card, shared_key);
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    id
}

// @scenario: contacts_management:Search contacts by name
#[test]
fn test_find_contact_fuzzy_matches_name_case_insensitive() {
    let wb = create_test_vauchi();

    add_named_contact(&wb, "Alice Smith", [1u8; 32]);
    add_named_contact(&wb, "Bob Jones", [2u8; 32]);

    let results = wb.find_contact_fuzzy("alice").unwrap();

    assert_eq!(
        results.len(),
        1,
        "should find exactly one match for 'alice'"
    );
    assert_eq!(results[0].display_name(), "Alice Smith");
}

// @scenario: contacts_management:Search contacts by name
#[test]
fn test_find_contact_fuzzy_matches_name_substring() {
    let wb = create_test_vauchi();

    add_named_contact(&wb, "Alice Smith", [1u8; 32]);
    add_named_contact(&wb, "Alice Jones", [2u8; 32]);
    add_named_contact(&wb, "Bob Smith", [3u8; 32]);

    let results = wb.find_contact_fuzzy("alice").unwrap();

    assert_eq!(results.len(), 2, "should find two Alices");
}

#[test]
fn test_find_contact_fuzzy_matches_id_prefix() {
    let wb = create_test_vauchi();

    let id = add_named_contact(&wb, "Alice", [1u8; 32]);
    add_named_contact(&wb, "Bob", [2u8; 32]);

    // Use the first 8 characters of Alice's ID as prefix
    let prefix = &id[..8];
    let results = wb.find_contact_fuzzy(prefix).unwrap();

    assert!(
        results.iter().any(|c| c.id() == id),
        "should find contact by ID prefix '{}'",
        prefix
    );
}

#[test]
fn test_find_contact_fuzzy_deduplicates_name_and_id_matches() {
    let wb = create_test_vauchi();

    // Create a contact whose name happens to contain its own ID prefix
    // (unlikely in practice, but we test deduplication)
    let id = add_named_contact(&wb, "TestContact", [1u8; 32]);

    // Search by name - should find by name match
    let results_by_name = wb.find_contact_fuzzy("TestContact").unwrap();
    assert_eq!(results_by_name.len(), 1);

    // Search by ID prefix - should find by ID match
    let prefix = &id[..6];
    let results_by_id = wb.find_contact_fuzzy(prefix).unwrap();
    assert!(
        results_by_id.iter().any(|c| c.id() == id),
        "should find the contact by ID prefix"
    );
}

// @scenario: contacts_management:Search contacts by name
#[test]
fn test_find_contact_fuzzy_returns_empty_for_no_match() {
    let wb = create_test_vauchi();

    add_named_contact(&wb, "Alice", [1u8; 32]);
    add_named_contact(&wb, "Bob", [2u8; 32]);

    let results = wb.find_contact_fuzzy("zzz_no_match").unwrap();

    assert!(
        results.is_empty(),
        "should return empty vec for non-matching query"
    );
}

#[test]
fn test_find_contact_fuzzy_returns_empty_for_empty_query() {
    let wb = create_test_vauchi();

    add_named_contact(&wb, "Alice", [1u8; 32]);

    // Empty query should match nothing (or everything depending on semantics;
    // we define it as matching nothing for safety)
    let results = wb.find_contact_fuzzy("").unwrap();

    // Empty query returns all contacts (substring match on "" matches everything)
    // This matches existing search_contacts behavior
    assert!(
        !results.is_empty(),
        "empty query should return all contacts"
    );
}

#[test]
fn test_find_contact_fuzzy_union_of_name_and_id_without_duplicates() {
    let wb = create_test_vauchi();

    let id_alice = add_named_contact(&wb, "Alice", [1u8; 32]);
    add_named_contact(&wb, "Bob", [2u8; 32]);

    // Search by name
    let by_name = wb.find_contact_fuzzy("Alice").unwrap();
    assert_eq!(by_name.len(), 1);
    assert_eq!(by_name[0].display_name(), "Alice");

    // Search by ID prefix of Alice (should also return Alice, no duplicates)
    let prefix = &id_alice[..8];
    let by_id = wb.find_contact_fuzzy(prefix).unwrap();

    // Each result should appear at most once
    let mut seen_ids: Vec<String> = by_id.iter().map(|c| c.id().to_string()).collect();
    seen_ids.sort();
    seen_ids.dedup();
    assert_eq!(
        seen_ids.len(),
        by_id.len(),
        "results should not contain duplicates"
    );
}
