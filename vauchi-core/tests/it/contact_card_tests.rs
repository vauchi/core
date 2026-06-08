// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact_card
//! Extracted from mod.rs

use vauchi_core::*;

// @scenario: contact_card_management :: Create contact card with display name
#[test]
fn test_create_card() {
    let card = ContactCard::new("Test User");
    assert_eq!(card.display_name(), "Test User");
    assert!(card.fields().is_empty());
}

// @scenario: contact_card_management :: Add field to contact card
// @scenario: contact_card_management :: Remove field from contact card
#[test]
fn test_add_and_remove_field() {
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Email, "Work", "test@test.com", 0);
    card.add_field(field).unwrap();
    assert_eq!(card.fields().len(), 1);

    let field_id = card.fields()[0].id().to_string();
    card.remove_field(&field_id).unwrap();
    assert!(card.fields().is_empty());
}

// W2 (2026-06-08-exchange-card-not-group-filtered): the group-filtered card
// chokepoint. filtered_to returns only allow-listed fields, leaves the
// original untouched (pure), and is safe under empty/superset allow-lists.
// @internal
#[test]
fn test_filtered_to_keeps_only_allowed_fields() {
    use std::collections::HashSet;
    let mut card = ContactCard::new("Owner");
    card.add_field(ContactField::new(FieldType::Email, "Work", "w@x.com", 0))
        .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+15551234567",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(FieldType::Email, "Home", "h@x.com", 0))
        .unwrap();

    let keep_a = card.fields()[0].id().to_string();
    let drop_b = card.fields()[1].id().to_string();
    let keep_c = card.fields()[2].id().to_string();
    let allow: HashSet<String> = HashSet::from([keep_a.clone(), keep_c.clone()]);

    let filtered = card.filtered_to(&allow);

    let ids: HashSet<String> = filtered
        .fields()
        .iter()
        .map(|f| f.id().to_string())
        .collect();
    assert_eq!(
        filtered.fields().len(),
        2,
        "only allow-listed fields remain"
    );
    assert!(ids.contains(&keep_a) && ids.contains(&keep_c));
    assert!(!ids.contains(&drop_b), "non-allow-listed field is dropped");

    // Purity: the source card is unchanged.
    assert_eq!(card.fields().len(), 3);

    // Empty allow-list → empty card (e.g. a group that shares nothing).
    assert!(card.filtered_to(&HashSet::new()).fields().is_empty());

    // Superset allow-list → all fields kept, no panic.
    let superset: HashSet<String> = HashSet::from([keep_a, drop_b, keep_c, "ghost".to_string()]);
    assert_eq!(card.filtered_to(&superset).fields().len(), 3);
}

// @scenario: onboarding_workflow :: Field visibility default empty (privacy-first)
#[test]
fn test_contact_card_field_visibility_default_empty() {
    let card = ContactCard::new("Alice");
    assert!(card.field_visibility().is_empty());
}

// @scenario: onboarding_workflow :: Show and hide field in no-group mode
#[test]
fn test_contact_card_show_hide_field() {
    let mut card = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Email, "Work", "alice@example.com", 0);
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();

    // Default: hidden (no explicit visibility rule)
    assert!(!card.is_field_shown(&field_id));

    // Show it
    card.set_field_shown(&field_id, true);
    assert!(card.is_field_shown(&field_id));

    // Hide it
    card.set_field_shown(&field_id, false);
    assert!(!card.is_field_shown(&field_id));
}

// @scenario: onboarding_workflow :: Remove field cleans up field_visibility
#[test]
fn test_remove_field_cleans_up_field_visibility() {
    let mut card = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1234567890", 0);
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    card.set_field_shown(&field_id, true);
    assert!(card.is_field_shown(&field_id));

    card.remove_field(&field_id).unwrap();
    assert!(!card.is_field_shown(&field_id));
    assert!(card.field_visibility().is_empty());
}

// @scenario: unicode_normalization :: Display name NFC normalization
#[test]
fn test_display_name_normalized_nfc() {
    let card = ContactCard::new("Jose\u{0301}");
    assert_eq!(card.display_name(), "Jos\u{00E9}");
}

// @scenario: unicode_normalization :: Display name trimmed
#[test]
fn test_display_name_trimmed() {
    let card = ContactCard::new("  Alice  ");
    assert_eq!(card.display_name(), "Alice");
}

// @scenario: unicode_normalization :: Whitespace-only display name rejected
#[test]
fn test_set_display_name_whitespace_only_rejected() {
    let mut card = ContactCard::new("Alice");
    let result = card.set_display_name("   ");
    assert!(result.is_err());
}
