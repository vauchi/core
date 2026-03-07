// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact_card
//! Extracted from mod.rs

use vauchi_core::*;

// @scenario: contact_card_management.feature:Create contact card with display name
#[test]
fn test_create_card() {
    let card = ContactCard::new("Test User");
    assert_eq!(card.display_name(), "Test User");
    assert!(card.fields().is_empty());
}

// @scenario: contact_card_management.feature:Add field to contact card
// @scenario: contact_card_management.feature:Remove field from contact card
#[test]
fn test_add_and_remove_field() {
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Email, "Work", "test@test.com");
    card.add_field(field).unwrap();
    assert_eq!(card.fields().len(), 1);

    let field_id = card.fields()[0].id().to_string();
    card.remove_field(&field_id).unwrap();
    assert!(card.fields().is_empty());
}

// @scenario: onboarding_workflow.feature:Shown fields default empty (privacy-first)
#[test]
fn test_contact_card_shown_fields_default_empty() {
    let card = ContactCard::new("Alice");
    assert!(card.shown_fields().is_empty());
}

// @scenario: onboarding_workflow.feature:Show and hide field in no-group mode
#[test]
fn test_contact_card_show_hide_field() {
    let mut card = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Email, "Work", "alice@example.com");
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();

    // Default: hidden (not in shown_fields)
    assert!(!card.is_field_shown(&field_id));

    // Show it
    card.set_field_shown(&field_id, true);
    assert!(card.is_field_shown(&field_id));

    // Hide it
    card.set_field_shown(&field_id, false);
    assert!(!card.is_field_shown(&field_id));
}

// @scenario: onboarding_workflow.feature:Remove field cleans up shown_fields
#[test]
fn test_remove_field_cleans_up_shown_fields() {
    let mut card = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1234567890");
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    card.set_field_shown(&field_id, true);
    assert!(card.is_field_shown(&field_id));

    card.remove_field(&field_id).unwrap();
    assert!(!card.is_field_shown(&field_id));
    assert!(!card.shown_fields().contains(&field_id));
}
