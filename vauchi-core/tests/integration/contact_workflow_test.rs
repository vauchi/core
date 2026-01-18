//! Contact Workflow Integration Tests
//!
//! Tests for contact management, visibility rules, and delta computation.

use vauchi_core::{
    network::MockTransport, Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi,
};

/// Test: Contact management workflow
#[test]
fn test_contact_management_workflow() {
    let wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

    // Initially no contacts
    assert_eq!(wb.contact_count().unwrap(), 0);

    // Create and add contacts
    let alice = Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    let bob = Contact::from_exchange([2u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    let carol = Contact::from_exchange(
        [3u8; 32],
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
    );

    let alice_id = alice.id().to_string();
    let bob_id = bob.id().to_string();

    wb.add_contact(alice).unwrap();
    wb.add_contact(bob).unwrap();
    wb.add_contact(carol).unwrap();

    // Verify contacts were added
    assert_eq!(wb.contact_count().unwrap(), 3);

    // List contacts
    let contacts = wb.list_contacts().unwrap();
    assert_eq!(contacts.len(), 3);

    // Get specific contact
    let alice_loaded = wb.get_contact(&alice_id).unwrap().unwrap();
    assert_eq!(alice_loaded.display_name(), "Alice");

    // Search contacts
    let results = wb.search_contacts("alice").unwrap();
    assert_eq!(results.len(), 1);

    let results = wb.search_contacts("bob").unwrap();
    assert_eq!(results.len(), 1);

    let results = wb.search_contacts("xyz").unwrap();
    assert_eq!(results.len(), 0);

    // Verify fingerprint
    wb.verify_contact_fingerprint(&alice_id).unwrap();
    let alice_loaded = wb.get_contact(&alice_id).unwrap().unwrap();
    assert!(alice_loaded.is_fingerprint_verified());

    // Remove contact
    let removed = wb.remove_contact(&bob_id).unwrap();
    assert!(removed);
    assert_eq!(wb.contact_count().unwrap(), 2);
    assert!(wb.get_contact(&bob_id).unwrap().is_none());
}

/// Test: Contact card delta computation and application
#[test]
fn test_card_delta_workflow() {
    use vauchi_core::sync::{CardDelta, FieldChange};

    // Create initial card
    let mut old_card = ContactCard::new("Test User");
    old_card
        .add_field(ContactField::new(FieldType::Email, "work", "old@work.com"))
        .unwrap();
    old_card
        .add_field(ContactField::new(
            FieldType::Phone,
            "mobile",
            "+15551234567",
        ))
        .unwrap();

    // Clone and modify card (to preserve field IDs for modification detection)
    let mut updated_card = old_card.clone();
    updated_card.set_display_name("Test User Updated").unwrap();
    // Modify the email value (same field ID)
    let email_field_id = updated_card.fields()[0].id().to_string();
    updated_card
        .update_field_value(&email_field_id, "new@work.com")
        .unwrap();
    // Remove mobile field
    let mobile_field_id = updated_card.fields()[1].id().to_string();
    updated_card.remove_field(&mobile_field_id).unwrap();
    // Add new field
    updated_card
        .add_field(ContactField::new(
            FieldType::Website,
            "blog",
            "https://blog.test.com",
        ))
        .unwrap();

    // Compute delta
    let delta = CardDelta::compute(&old_card, &updated_card);

    // Should have multiple changes
    assert!(!delta.changes.is_empty());

    // Display name changed
    assert!(delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::DisplayNameChanged { .. })));

    // Email modified (same field ID, different value)
    assert!(delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Modified { .. })));

    // Mobile removed
    assert!(delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Removed { .. })));

    // Blog added
    assert!(delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Added { .. })));

    // Apply delta to a copy of old card
    let mut result_card = old_card.clone();
    delta.apply(&mut result_card).unwrap();

    // Verify result matches updated card
    assert_eq!(result_card.display_name(), updated_card.display_name());
    assert_eq!(result_card.fields().len(), updated_card.fields().len());
}

/// Test: Error handling for contacts
#[test]
fn test_contact_error_handling() {
    let mut wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

    // Try to get public ID without identity
    let result = wb.public_id();
    assert!(result.is_err());

    // Create identity
    wb.create_identity("Test").unwrap();

    // Try to create identity again
    let result = wb.create_identity("Test2");
    assert!(result.is_err());

    // Try to get non-existent contact
    let result = wb.get_contact("nonexistent").unwrap();
    assert!(result.is_none());

    // Try to remove non-existent contact
    let result = wb.remove_contact("nonexistent").unwrap();
    assert!(!result);

    // Try to verify fingerprint for non-existent contact
    let result = wb.verify_contact_fingerprint("nonexistent");
    assert!(result.is_err());
}
