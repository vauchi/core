// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact card gaps: shareable export, avatar sync delta, field order persistence.
//!
//! Feature tags: @contacts @shareable @avatar @sync @field-order

use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::sync::CardDelta;

// ============================================================================
// Shareable Card Export
// ============================================================================

/// Feature: contacts_management.feature @shareable
/// Scenario: Export card as shareable plain text (no exchange required)
#[test]
fn test_to_shareable_text_includes_name_and_fields() {
    let mut card = ContactCard::new("Alice Smith");
    card.add_field(ContactField::new(FieldType::Phone, "Mobile", "+1-555-1234"))
        .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "alice@example.com",
    ))
    .unwrap();

    let text = card.to_shareable_text();

    assert!(text.contains("Alice Smith"), "Should contain display name");
    assert!(text.contains("+1-555-1234"), "Should contain phone");
    assert!(text.contains("alice@example.com"), "Should contain email");
}

/// Feature: contacts_management.feature @shareable
/// Scenario: Shareable text has no internal IDs or crypto data
#[test]
fn test_to_shareable_text_excludes_internal_data() {
    let mut card = ContactCard::new("Bob");
    card.add_field(ContactField::new(FieldType::Phone, "Home", "+1-555-5678"))
        .unwrap();

    let text = card.to_shareable_text();

    // Should not contain internal card ID (hex string)
    assert!(
        !text.contains(card.id()),
        "Should not expose internal card ID"
    );
}

/// Feature: contacts_management.feature @shareable
/// Scenario: Shareable QR data is compact and parseable
#[test]
fn test_to_shareable_qr_data_roundtrip() {
    let mut card = ContactCard::new("Charlie");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Personal",
        "charlie@test.com",
    ))
    .unwrap();

    let qr_data = card.to_shareable_qr_data();

    // Should be valid JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&qr_data).expect("QR data should be valid JSON");

    assert_eq!(parsed["name"], "Charlie");
    assert!(parsed["fields"].is_array());
}

/// Feature: contacts_management.feature @shareable
/// Scenario: Empty card produces minimal shareable text
#[test]
fn test_to_shareable_text_empty_card() {
    let card = ContactCard::new("Dave");

    let text = card.to_shareable_text();

    assert!(text.contains("Dave"));
    // Should still work with no fields
    assert!(!text.is_empty());
}

// ============================================================================
// Avatar Removal Sync
// ============================================================================

/// Feature: sync_updates.feature @avatar @sync
/// Scenario: Avatar removal is captured as a delta change
#[test]
fn test_avatar_removal_captured_in_delta() {
    let mut old_card = ContactCard::new("Eve");
    old_card.set_avatar(vec![0xFF, 0xD8, 0xFF]).unwrap(); // Fake JPEG header

    let mut new_card = old_card.clone();
    new_card.clear_avatar();

    let delta = CardDelta::compute(&old_card, &new_card);

    // Delta should capture the avatar removal
    assert!(
        delta.has_avatar_change(),
        "Delta should detect avatar removal"
    );
}

/// Feature: sync_updates.feature @avatar @sync
/// Scenario: Avatar addition is captured as a delta change
#[test]
fn test_avatar_addition_captured_in_delta() {
    let old_card = ContactCard::new("Frank");

    let mut new_card = old_card.clone();
    new_card.set_avatar(vec![0x89, 0x50, 0x4E, 0x47]).unwrap(); // Fake PNG header

    let delta = CardDelta::compute(&old_card, &new_card);

    assert!(
        delta.has_avatar_change(),
        "Delta should detect avatar addition"
    );
}

/// Feature: sync_updates.feature @avatar @sync
/// Scenario: No delta when avatar is unchanged
#[test]
fn test_no_delta_when_avatar_unchanged() {
    let mut card = ContactCard::new("Grace");
    card.set_avatar(vec![0x01, 0x02, 0x03]).unwrap();

    let same_card = card.clone();

    let delta = CardDelta::compute(&card, &same_card);

    assert!(
        !delta.has_avatar_change(),
        "No avatar change when unchanged"
    );
    assert!(
        delta.changes.is_empty(),
        "No changes at all when cards are identical"
    );
}

// ============================================================================
// Field Order Persistence
// ============================================================================

/// Feature: contacts_management.feature @field-order
/// Scenario: Field order survives JSON serialization round-trip
#[test]
fn test_field_order_survives_serialization() {
    let mut card = ContactCard::new("Heidi");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+15551110000",
    ))
    .unwrap();
    card.add_field(ContactField::new(FieldType::Email, "Work", "h@test.com"))
        .unwrap();
    card.add_field(ContactField::new(FieldType::Phone, "Home", "+15552220000"))
        .unwrap();

    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();

    // Field order must be preserved
    let original_labels: Vec<&str> = card.fields().iter().map(|f| f.label()).collect();
    let restored_labels: Vec<&str> = restored.fields().iter().map(|f| f.label()).collect();
    assert_eq!(
        original_labels, restored_labels,
        "Field order must survive serialization"
    );
}

/// Feature: contacts_management.feature @field-order
/// Scenario: Reordered fields persist after serialization
#[test]
fn test_reordered_fields_persist_after_serialization() {
    let mut card = ContactCard::new("Ivan");
    card.add_field(ContactField::new(FieldType::Email, "A", "a@test.com"))
        .unwrap();
    card.add_field(ContactField::new(FieldType::Email, "B", "b@test.com"))
        .unwrap();
    card.add_field(ContactField::new(FieldType::Email, "C", "c@test.com"))
        .unwrap();

    let field_ids: Vec<String> = card.fields().iter().map(|f| f.id().to_string()).collect();
    // Reorder: C, A, B
    card.reorder_fields(&[&field_ids[2], &field_ids[0], &field_ids[1]])
        .unwrap();

    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();

    let labels: Vec<&str> = restored.fields().iter().map(|f| f.label()).collect();
    assert_eq!(labels, vec!["C", "A", "B"], "Reordered fields must persist");
}
