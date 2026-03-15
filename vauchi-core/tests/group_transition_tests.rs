// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for group transition logic (no-group <-> groups mode).
//!
//! When the last group is deleted, visible fields migrate to field_visibility
//! on ContactCard (transition to no-group mode). When the first group is
//! created, field_visibility are preserved for the user to explicitly reassign.

use vauchi_core::{ContactField, FieldType, Vauchi};

fn create_vauchi_with_identity(name: &str) -> Vauchi {
    let mut wb: Vauchi = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb
}

// @scenario: visibility_control.feature:Delete last group migrates fields to field_visibility
#[test]
fn test_transition_to_no_group_mode() {
    let wb = create_vauchi_with_identity("Alice");

    // Add a field to own card
    let field = ContactField::new(FieldType::Email, "Email", "alice@example.com");
    wb.add_own_field(field).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    let field_id = card.fields()[0].id().to_string();

    // Create a label and set the field visible in it
    let label = wb.create_group("Friends").unwrap();
    let label_id = label.id().to_string();
    wb.set_group_field_visibility(&label_id, &field_id, true)
        .unwrap();

    // Verify field is visible in label
    let label = wb.get_group(&label_id).unwrap();
    assert!(
        label.is_field_visible(&field_id),
        "field should be visible in label before deletion"
    );

    // Verify field is NOT in field_visibility before deletion
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        !card.is_field_shown(&field_id),
        "field should not be in field_visibility while groups exist"
    );

    // Delete the last label — triggers transition to no-group mode
    wb.delete_group(&label_id).unwrap();

    // Verify no labels remain
    let labels = wb.list_groups().unwrap();
    assert_eq!(labels.len(), 0, "all labels should be deleted");

    // Verify field has been migrated to field_visibility on the card
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        card.is_field_shown(&field_id),
        "field should be migrated to field_visibility after last group deleted"
    );
}

// @scenario: visibility_control.feature:Creating first group preserves field_visibility
#[test]
fn test_transition_preserves_field_visibility_when_adding_first_group() {
    let wb = create_vauchi_with_identity("Alice");

    // Add a field to own card
    let field = ContactField::new(FieldType::Email, "Email", "alice@example.com");
    wb.add_own_field(field).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    let field_id = card.fields()[0].id().to_string();

    // Set field as shown (no-group mode)
    wb.set_field_shown(&field_id, true).unwrap();

    // Verify field is in field_visibility
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        card.is_field_shown(&field_id),
        "field should be shown in no-group mode"
    );

    // Create first group — field_visibility should be preserved
    let _label = wb.create_group("Work").unwrap();

    // Verify field_visibility still has the field (user must manually reassign)
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        card.is_field_shown(&field_id),
        "field_visibility should be preserved when first group is created"
    );
}

// @scenario: visibility_control.feature:Delete non-last group does not migrate
#[test]
fn test_delete_non_last_label_no_migration() {
    let wb = create_vauchi_with_identity("Alice");

    // Add a field to own card
    let field = ContactField::new(FieldType::Email, "Email", "alice@example.com");
    wb.add_own_field(field).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    let field_id = card.fields()[0].id().to_string();

    // Create two labels
    let label1 = wb.create_group("Friends").unwrap();
    let label1_id = label1.id().to_string();
    let _label2 = wb.create_group("Work").unwrap();

    // Set field visible in label1
    wb.set_group_field_visibility(&label1_id, &field_id, true)
        .unwrap();

    // Verify field is NOT in field_visibility
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        !card.is_field_shown(&field_id),
        "field should not be in field_visibility while groups exist"
    );

    // Delete label1 (not the last one — label2 still exists)
    wb.delete_group(&label1_id).unwrap();

    // Verify no migration to field_visibility (still in groups mode)
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        !card.is_field_shown(&field_id),
        "should not migrate to field_visibility when other groups still exist"
    );

    // Verify label2 still exists
    let labels = wb.list_groups().unwrap();
    assert_eq!(labels.len(), 1, "one label should remain");
}

// @scenario: visibility_control.feature:set_field_shown API persists changes
#[test]
fn test_set_field_shown_api() {
    let wb = create_vauchi_with_identity("Alice");

    // Add a field to own card
    let field = ContactField::new(FieldType::Email, "Email", "alice@example.com");
    wb.add_own_field(field).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    let field_id = card.fields()[0].id().to_string();

    // Initially not shown
    assert!(
        !card.is_field_shown(&field_id),
        "field should not be shown initially"
    );

    // Set field shown
    wb.set_field_shown(&field_id, true).unwrap();

    // Verify it's shown (reload from storage)
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        card.is_field_shown(&field_id),
        "field should be shown after set_field_shown(true)"
    );

    // Set field hidden again
    wb.set_field_shown(&field_id, false).unwrap();

    // Verify it's hidden
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        !card.is_field_shown(&field_id),
        "field should not be shown after set_field_shown(false)"
    );
}

// @scenario: visibility_control.feature:Delete last group with multiple visible fields
#[test]
fn test_transition_migrates_all_visible_fields() {
    let wb = create_vauchi_with_identity("Alice");

    // Add multiple fields
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "Email",
        "alice@example.com",
    ))
    .unwrap();
    wb.add_own_field(ContactField::new(FieldType::Phone, "Phone", "+1234567890"))
        .unwrap();

    let card = wb.own_card().unwrap().unwrap();
    let email_id = card.fields()[0].id().to_string();
    let phone_id = card.fields()[1].id().to_string();

    // Create label and set both fields visible
    let label = wb.create_group("Friends").unwrap();
    let label_id = label.id().to_string();
    wb.set_group_field_visibility(&label_id, &email_id, true)
        .unwrap();
    wb.set_group_field_visibility(&label_id, &phone_id, true)
        .unwrap();

    // Delete last label
    wb.delete_group(&label_id).unwrap();

    // Both fields should be migrated to field_visibility
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        card.is_field_shown(&email_id),
        "email should be migrated to field_visibility"
    );
    assert!(
        card.is_field_shown(&phone_id),
        "phone should be migrated to field_visibility"
    );
}

// @scenario: visibility_control.feature:Delete last group does not migrate hidden fields
#[test]
fn test_transition_does_not_migrate_hidden_fields() {
    let wb = create_vauchi_with_identity("Alice");

    // Add two fields
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "Email",
        "alice@example.com",
    ))
    .unwrap();
    wb.add_own_field(ContactField::new(FieldType::Phone, "Phone", "+1234567890"))
        .unwrap();

    let card = wb.own_card().unwrap().unwrap();
    let email_id = card.fields()[0].id().to_string();
    let phone_id = card.fields()[1].id().to_string();

    // Create label but only set email visible (phone stays hidden)
    let label = wb.create_group("Friends").unwrap();
    let label_id = label.id().to_string();
    wb.set_group_field_visibility(&label_id, &email_id, true)
        .unwrap();

    // Delete last label
    wb.delete_group(&label_id).unwrap();

    // Only email should be migrated; phone was never visible
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        card.is_field_shown(&email_id),
        "visible email should be migrated to field_visibility"
    );
    assert!(
        !card.is_field_shown(&phone_id),
        "hidden phone should NOT be migrated to field_visibility"
    );
}
