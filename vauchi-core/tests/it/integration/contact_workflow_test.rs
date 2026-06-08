// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Workflow Integration Tests
//!
//! Tests for contact management, visibility rules, and delta computation.

use vauchi_core::{Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi};

/// Test: Contact management workflow
// @internal
#[test]
fn test_contact_management_workflow() {
    let wb: Vauchi = Vauchi::in_memory().unwrap();

    assert_eq!(wb.contact_count().unwrap(), 0);

    let alice = Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    let bob = Contact::from_exchange(
        [2u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let carol = Contact::from_exchange(
        [3u8; 32],
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
        0,
    );

    let alice_id = alice.id().to_string();
    let bob_id = bob.id().to_string();

    wb.add_contact(alice).unwrap();
    wb.add_contact(bob).unwrap();
    wb.add_contact(carol).unwrap();

    assert_eq!(wb.contact_count().unwrap(), 3);

    let contacts = wb.list_contacts().unwrap();
    assert_eq!(contacts.len(), 3);

    let alice_loaded = wb.get_contact(&alice_id).unwrap().unwrap();
    assert_eq!(alice_loaded.display_name(), "Alice");

    let results = wb.search_contacts("alice").unwrap();
    assert_eq!(results.len(), 1);

    let results = wb.search_contacts("bob").unwrap();
    assert_eq!(results.len(), 1);

    let results = wb.search_contacts("xyz").unwrap();
    assert_eq!(results.len(), 0);

    wb.verify_contact_fingerprint(&alice_id).unwrap();
    let alice_loaded = wb.get_contact(&alice_id).unwrap().unwrap();
    assert!(alice_loaded.is_fingerprint_verified());

    let removed = wb.remove_contact(&bob_id).unwrap();
    assert!(removed);
    assert_eq!(wb.contact_count().unwrap(), 2);
    assert!(wb.get_contact(&bob_id).unwrap().is_none());
}

/// Test: Contact card delta computation and application
// @internal
#[test]
fn test_card_delta_workflow() {
    use vauchi_core::sync::{CardDelta, FieldChange};

    let mut old_card = ContactCard::new("Test User");
    old_card
        .add_field(ContactField::new(
            FieldType::Email,
            "work",
            "old@work.com",
            0,
        ))
        .unwrap();
    old_card
        .add_field(ContactField::new(
            FieldType::Phone,
            "mobile",
            "+15551234567",
            0,
        ))
        .unwrap();

    // Clone and modify card (to preserve field IDs for modification detection)
    let mut updated_card = old_card.clone();
    updated_card.set_display_name("Test User Updated").unwrap();
    // Modify the email value (same field ID)
    let email_field_id = updated_card.fields()[0].id().to_string();
    updated_card
        .update_field_value(&email_field_id, "new@work.com", 0)
        .unwrap();
    let mobile_field_id = updated_card.fields()[1].id().to_string();
    updated_card.remove_field(&mobile_field_id).unwrap();
    updated_card
        .add_field(ContactField::new(
            FieldType::Website,
            "blog",
            "https://blog.test.com",
            0,
        ))
        .unwrap();

    let delta = CardDelta::compute(&old_card, &updated_card, 0);

    assert!(!delta.changes.is_empty());

    assert!(
        delta
            .changes
            .iter()
            .any(|c| matches!(c, FieldChange::DisplayNameChanged { .. }))
    );

    // Email modified (same field ID, different value)
    assert!(
        delta
            .changes
            .iter()
            .any(|c| matches!(c, FieldChange::Modified { .. }))
    );

    assert!(
        delta
            .changes
            .iter()
            .any(|c| matches!(c, FieldChange::Removed { .. }))
    );

    assert!(
        delta
            .changes
            .iter()
            .any(|c| matches!(c, FieldChange::Added { .. }))
    );

    let mut result_card = old_card.clone();
    delta.apply(&mut result_card, 0).unwrap();

    assert_eq!(result_card.display_name(), updated_card.display_name());
    assert_eq!(result_card.fields().len(), updated_card.fields().len());
}

/// Test: Error handling for contacts
// @internal
#[test]
fn test_contact_error_handling() {
    let mut wb: Vauchi = Vauchi::in_memory().unwrap();

    let result = wb.public_id();
    result.expect_err("expected error");

    wb.create_identity("Test").unwrap();

    let result = wb.create_identity("Test2");
    result.expect_err("expected error");

    let result = wb.get_contact("nonexistent").unwrap();
    assert!(result.is_none());

    let result = wb.remove_contact("nonexistent").unwrap();
    assert!(!result);

    let result = wb.verify_contact_fingerprint("nonexistent");
    result.expect_err("expected error");
}
