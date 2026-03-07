// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility Resolution Tests
//!
//! Tests for the two-mode visibility resolution logic:
//! - No-group mode (no labels): uses card's shown_fields
//! - Groups mode (labels exist): uses label-based visibility via LabelManager
//!
//! Traces to: features/visibility_labels.feature
//! - @no-group-mode: shown_fields-based visibility
//! - @groups-mode: label-based visibility
//! - @ungrouped-contact: default-closed for ungrouped contacts in groups mode

use vauchi_core::contact::labels::resolve_visible_fields;
use vauchi_core::contact::LabelManager;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};

#[test]
fn test_visible_fields_no_groups_mode() {
    let mut card = ContactCard::new("Alice");
    let field1 = ContactField::new(FieldType::Email, "Work", "alice@example.com");
    let field1_id = field1.id().to_string();
    let field2 = ContactField::new(FieldType::Phone, "Mobile", "+1234567890");
    let field2_id = field2.id().to_string();
    card.add_field(field1).unwrap();
    card.add_field(field2).unwrap();

    // Mark only field1 as shown
    card.set_field_shown(&field1_id, true);

    let label_manager = LabelManager::new();
    let visible = resolve_visible_fields(&card, &label_manager, "contact-123");

    assert!(visible.contains(&field1_id));
    assert!(!visible.contains(&field2_id));
    assert_eq!(visible.len(), 1);
}

#[test]
fn test_visible_fields_no_groups_mode_empty() {
    let mut card = ContactCard::new("Alice");
    let field1 = ContactField::new(FieldType::Email, "Work", "alice@example.com");
    card.add_field(field1).unwrap();

    // No fields marked as shown
    let label_manager = LabelManager::new();
    let visible = resolve_visible_fields(&card, &label_manager, "contact-123");

    assert!(visible.is_empty(), "No shown fields means no visibility");
}

#[test]
fn test_visible_fields_groups_mode() {
    let mut card = ContactCard::new("Alice");
    let field1 = ContactField::new(FieldType::Email, "Work", "alice@example.com");
    let field1_id = field1.id().to_string();
    card.add_field(field1).unwrap();
    card.set_field_shown(&field1_id, true); // shown in no-group mode

    let mut label_manager = LabelManager::new();
    let label = label_manager.create_label("Friends").unwrap();
    let label_id = label.id().to_string();

    // Add contact to Friends and make field1 visible via label
    label_manager
        .add_contact_to_label(&label_id, "contact-123")
        .unwrap();
    let label = label_manager.get_label_mut(&label_id).unwrap();
    label.add_visible_field(&field1_id);

    let visible = resolve_visible_fields(&card, &label_manager, "contact-123");
    assert!(visible.contains(&field1_id));
}

#[test]
fn test_visible_fields_ungrouped_contact_in_groups_mode() {
    let mut card = ContactCard::new("Alice");
    let field1 = ContactField::new(FieldType::Email, "Work", "alice@example.com");
    let field1_id = field1.id().to_string();
    card.add_field(field1).unwrap();
    card.set_field_shown(&field1_id, true);

    let mut label_manager = LabelManager::new();
    label_manager.create_label("Friends").unwrap();

    // "ungrouped-contact" is not in any label
    let visible = resolve_visible_fields(&card, &label_manager, "ungrouped-contact");
    assert!(
        visible.is_empty(),
        "Ungrouped contacts in groups mode see no fields"
    );
}

#[test]
fn test_visible_fields_groups_mode_ignores_shown_fields() {
    let mut card = ContactCard::new("Alice");
    let field1 = ContactField::new(FieldType::Email, "Work", "alice@example.com");
    let field1_id = field1.id().to_string();
    let field2 = ContactField::new(FieldType::Phone, "Mobile", "+1234567890");
    let field2_id = field2.id().to_string();
    card.add_field(field1).unwrap();
    card.add_field(field2).unwrap();

    // Mark both fields as shown (no-group mode)
    card.set_field_shown(&field1_id, true);
    card.set_field_shown(&field2_id, true);

    let mut label_manager = LabelManager::new();
    let label = label_manager.create_label("Friends").unwrap();
    let label_id = label.id().to_string();

    // Only make field1 visible via label — field2 should NOT be visible
    // even though it's in shown_fields
    label_manager
        .add_contact_to_label(&label_id, "contact-123")
        .unwrap();
    let label = label_manager.get_label_mut(&label_id).unwrap();
    label.add_visible_field(&field1_id);

    let visible = resolve_visible_fields(&card, &label_manager, "contact-123");
    assert!(visible.contains(&field1_id));
    assert!(
        !visible.contains(&field2_id),
        "Groups mode should use label visibility, not shown_fields"
    );
    assert_eq!(visible.len(), 1);
}

#[test]
fn test_visible_fields_groups_mode_with_per_contact_override() {
    let mut card = ContactCard::new("Alice");
    let field1 = ContactField::new(FieldType::Email, "Work", "alice@example.com");
    let field1_id = field1.id().to_string();
    let field2 = ContactField::new(FieldType::Phone, "Mobile", "+1234567890");
    let field2_id = field2.id().to_string();
    card.add_field(field1).unwrap();
    card.add_field(field2).unwrap();

    let mut label_manager = LabelManager::new();
    let label = label_manager.create_label("Friends").unwrap();
    let label_id = label.id().to_string();

    label_manager
        .add_contact_to_label(&label_id, "contact-123")
        .unwrap();
    let label = label_manager.get_label_mut(&label_id).unwrap();
    label.add_visible_field(&field1_id);

    // Per-contact override: hide field1, show field2
    label_manager.set_contact_override("contact-123", &field1_id, false);
    label_manager.set_contact_override("contact-123", &field2_id, true);

    let visible = resolve_visible_fields(&card, &label_manager, "contact-123");
    assert!(
        !visible.contains(&field1_id),
        "Per-contact override should hide field1"
    );
    assert!(
        visible.contains(&field2_id),
        "Per-contact override should show field2"
    );
}

#[test]
fn test_is_empty_label_manager() {
    let mut manager = LabelManager::new();
    assert!(manager.is_empty());

    manager.create_label("Friends").unwrap();
    assert!(!manager.is_empty());
}
