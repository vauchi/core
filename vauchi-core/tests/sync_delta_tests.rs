// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for sync::delta
//! Extracted from delta.rs

use vauchi_core::contact_card::FieldType;
use vauchi_core::sync::*;
use vauchi_core::*;

#[test]
fn test_delta_compute_no_changes() {
    let card = ContactCard::new("Alice");
    let delta = CardDelta::compute(&card, &card);

    assert!(delta.is_empty());
}

#[test]
fn test_delta_compute_display_name_change() {
    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Smith");

    let delta = CardDelta::compute(&old, &new);

    assert_eq!(delta.changes.len(), 1);
    assert!(matches!(
        &delta.changes[0],
        FieldChange::DisplayNameChanged { new_name } if new_name == "Alice Smith"
    ));
}

#[test]
fn test_delta_compute_field_added() {
    let old = ContactCard::new("Alice");

    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ));

    let delta = CardDelta::compute(&old, &new);

    assert_eq!(delta.changes.len(), 1);
    assert!(matches!(&delta.changes[0], FieldChange::Added { .. }));
}

#[test]
fn test_delta_compute_field_modified() {
    let mut old = ContactCard::new("Alice");
    let _ = old.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
    ));

    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
    ));

    let delta = CardDelta::compute(&old, &new);

    // The field IDs are generated, so both have different IDs
    // This will show as added + removed rather than modified
    // For true modification tracking, we'd need stable field IDs
    assert!(!delta.is_empty());
}

#[test]
fn test_delta_compute_field_removed() {
    let mut old = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Email, "email", "alice@example.com");
    let field_id = field.id().to_string();
    let _ = old.add_field(field);

    let new = ContactCard::new("Alice");

    let delta = CardDelta::compute(&old, &new);

    assert_eq!(delta.changes.len(), 1);
    assert!(matches!(
        &delta.changes[0],
        FieldChange::Removed { field_id: id } if *id == field_id
    ));
}

#[test]
fn test_delta_apply_display_name() {
    let mut card = ContactCard::new("Alice");

    let delta = CardDelta {
        version: 1,
        timestamp: 12345,
        changes: vec![FieldChange::DisplayNameChanged {
            new_name: "Alice Smith".to_string(),
        }],
        nonce: [0u8; 32],
        signature: [0u8; 64],
    };

    delta.apply(&mut card).unwrap();

    assert_eq!(card.display_name(), "Alice Smith");
}

#[test]
fn test_delta_apply_add_field() {
    let mut card = ContactCard::new("Alice");
    let new_field = ContactField::new(FieldType::Email, "email", "alice@example.com");

    let delta = CardDelta {
        version: 1,
        timestamp: 12345,
        changes: vec![FieldChange::Added { field: new_field }],
        nonce: [0u8; 32],
        signature: [0u8; 64],
    };

    delta.apply(&mut card).unwrap();

    assert_eq!(card.fields().len(), 1);
    assert_eq!(card.fields()[0].value(), "alice@example.com");
}

#[test]
fn test_delta_apply_remove_field() {
    let mut card = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Email, "email", "alice@example.com");
    let field_id = field.id().to_string();
    let _ = card.add_field(field);

    let delta = CardDelta {
        version: 1,
        timestamp: 12345,
        changes: vec![FieldChange::Removed { field_id }],
        nonce: [0u8; 32],
        signature: [0u8; 64],
    };

    delta.apply(&mut card).unwrap();

    assert!(card.fields().is_empty());
}

#[test]
fn test_delta_roundtrip() {
    let mut old = ContactCard::new("Alice");
    let _ = old.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

    let mut new = ContactCard::new("Alice Smith");
    let _ = new.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ));

    let delta = CardDelta::compute(&old, &new);

    // Apply to a copy of old
    let mut result = old.clone();
    delta.apply(&mut result).unwrap();

    assert_eq!(result.display_name(), "Alice Smith");
    assert_eq!(result.fields().len(), 2);
}

#[test]
fn test_delta_sign_and_verify() {
    let identity = Identity::create("Test User");

    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Smith");

    let mut delta = CardDelta::compute(&old, &new);
    delta.sign(&identity);

    // Verify with correct public key
    assert!(delta.verify(identity.signing_public_key()));

    // Verify with wrong public key should fail
    let other_identity = Identity::create("Other User");
    assert!(!delta.verify(other_identity.signing_public_key()));
}

#[test]
fn test_delta_serialization_roundtrip() {
    let mut old = ContactCard::new("Alice");
    let _ = old.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
    ));

    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
    ));

    let delta = CardDelta::compute(&old, &new);

    let json = serde_json::to_string(&delta).unwrap();
    let restored: CardDelta = serde_json::from_str(&json).unwrap();

    assert_eq!(delta.version, restored.version);
    assert_eq!(delta.timestamp, restored.timestamp);
    assert_eq!(delta.changes.len(), restored.changes.len());
}

#[test]
fn test_delta_multiple_changes() {
    let mut old = ContactCard::new("Alice");
    let field1 = ContactField::new(FieldType::Email, "email", "alice@example.com");
    let field1_id = field1.id().to_string();
    let _ = old.add_field(field1);

    let mut new = ContactCard::new("Alice Smith");
    // email field is removed, phone is added
    let _ = new.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

    let delta = CardDelta::compute(&old, &new);

    // Should have: DisplayNameChanged, Removed (email), Added (phone)
    assert_eq!(delta.changes.len(), 3);

    let has_name_change = delta.changes.iter().any(
        |c| matches!(c, FieldChange::DisplayNameChanged { new_name } if new_name == "Alice Smith"),
    );
    assert!(has_name_change);

    let has_removed = delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Removed { field_id } if *field_id == field1_id));
    assert!(has_removed);

    let has_added = delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Added { .. }));
    assert!(has_added);
}

#[test]
fn test_delta_filter_for_contact_all_visible() {
    use vauchi_core::contact::VisibilityRules;

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ));
    let _ = new.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

    let delta = CardDelta::compute(&old, &new);
    let rules = VisibilityRules::new(); // Default: everyone can see all

    let filtered = delta.filter_for_contact("bob", &rules);

    // Bob should see both fields (default visibility is Everyone)
    assert_eq!(filtered.changes.len(), 2);
}

#[test]
fn test_delta_filter_for_contact_some_hidden() {
    use vauchi_core::contact::VisibilityRules;

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let email_field = ContactField::new(FieldType::Email, "email", "alice@example.com");
    let email_id = email_field.id().to_string();
    let _ = new.add_field(email_field);
    let _ = new.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

    let delta = CardDelta::compute(&old, &new);

    // Hide email from Bob
    let mut rules = VisibilityRules::new();
    rules.set_nobody(&email_id);

    let filtered = delta.filter_for_contact("bob", &rules);

    // Bob should only see the phone field
    assert_eq!(filtered.changes.len(), 1);
    assert!(
        matches!(&filtered.changes[0], FieldChange::Added { field } if field.label() == "phone")
    );
}

#[test]
fn test_delta_filter_for_contact_restricted_access() {
    use std::collections::HashSet;
    use vauchi_core::contact::VisibilityRules;

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let email_field = ContactField::new(FieldType::Email, "email", "alice@example.com");
    let email_id = email_field.id().to_string();
    let _ = new.add_field(email_field);

    let delta = CardDelta::compute(&old, &new);

    // Email only visible to specific contacts
    let mut rules = VisibilityRules::new();
    let mut allowed = HashSet::new();
    allowed.insert("charlie".to_string());
    rules.set_contacts(&email_id, allowed);

    // Bob is not in the allowed list
    let bob_filtered = delta.filter_for_contact("bob", &rules);
    assert!(bob_filtered.is_empty());

    // Charlie is in the allowed list
    let charlie_filtered = delta.filter_for_contact("charlie", &rules);
    assert_eq!(charlie_filtered.changes.len(), 1);
}

#[test]
fn test_delta_filter_display_name_always_visible() {
    use vauchi_core::contact::VisibilityRules;

    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Smith");

    let delta = CardDelta::compute(&old, &new);
    let rules = VisibilityRules::new();

    let filtered = delta.filter_for_contact("bob", &rules);

    // Display name changes are always visible
    assert_eq!(filtered.changes.len(), 1);
    assert!(matches!(
        &filtered.changes[0],
        FieldChange::DisplayNameChanged { .. }
    ));
}
