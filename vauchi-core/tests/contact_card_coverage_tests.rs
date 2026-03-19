// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Additional ContactCard tests for coverage of set_display_name, update_field,
//! remove_field, validate_size, reorder_fields, avatar methods.

use vauchi_core::{ContactCard, ContactField, FieldType};

// @scenario: contact_card_management:Update display name
#[test]
fn test_set_display_name() {
    let mut card = ContactCard::new("Original");
    card.set_display_name("Updated").unwrap();
    assert_eq!(card.display_name(), "Updated");
}

// @scenario: contact_card_management:Display name cannot be empty
#[test]
fn test_set_display_name_empty_fails() {
    let mut card = ContactCard::new("Original");
    let result = card.set_display_name("");
    result.expect_err("expected error");
}

// @scenario: contact_card_management:Display name length limit
#[test]
fn test_set_display_name_too_long_fails() {
    let mut card = ContactCard::new("Original");
    let long = "X".repeat(101);
    let result = card.set_display_name(&long);
    result.expect_err("expected error");
}

// @scenario: contact_card_management:Display name length limit
#[test]
fn test_set_display_name_max_length() {
    let mut card = ContactCard::new("Original");
    let exactly_100 = "X".repeat(100);
    card.set_display_name(&exactly_100).unwrap();
    assert_eq!(card.display_name(), exactly_100);
}

// @scenario: contact_card_management:Edit an existing field value
#[test]
fn test_update_field_value() {
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Email, "work", "old@test.com");
    card.add_field(field).unwrap();

    let field_id = card.fields()[0].id().to_string();
    card.update_field_value(&field_id, "new@test.com").unwrap();
    assert_eq!(card.fields()[0].value(), "new@test.com");
}

#[test]
fn test_update_field_value_not_found() {
    let mut card = ContactCard::new("Test");
    let result = card.update_field_value("nonexistent", "value");
    result.expect_err("expected error");
}

// @scenario: contact_card_management:Edit a field label
#[test]
fn test_update_field_label() {
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Email, "work", "test@test.com");
    card.add_field(field).unwrap();

    let field_id = card.fields()[0].id().to_string();
    card.update_field_label(&field_id, "personal").unwrap();
    assert_eq!(card.fields()[0].label(), "personal");
}

#[test]
fn test_update_field_label_not_found() {
    let mut card = ContactCard::new("Test");
    let result = card.update_field_label("nonexistent", "label");
    result.expect_err("expected error");
}

#[test]
fn test_remove_field_not_found() {
    let mut card = ContactCard::new("Test");
    let result = card.remove_field("nonexistent");
    result.expect_err("expected error");
}

// @scenario: contact_card_management:Exceed maximum fields
// @scenario: contact_card_management.feature:Maximum number of fields
#[test]
fn test_max_fields_reached() {
    let mut card = ContactCard::new("Test");
    for i in 0..vauchi_core::contact_card::MAX_FIELDS {
        card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field_{}", i),
            &format!("value_{}", i),
        ))
        .unwrap();
    }
    let result = card.add_field(ContactField::new(FieldType::Custom, "extra", "value"));
    result.expect_err("expected error");
}

// @scenario: contact_card_management:Contact card size limit
#[test]
fn test_validate_size_ok() {
    let card = ContactCard::new("Test");
    assert!(
        card.validate_size().is_ok(),
        "Default card should pass size validation"
    );
}

// @scenario: contact_card_management:Reorder contact fields
#[test]
fn test_reorder_fields() {
    let mut card = ContactCard::new("Test");
    card.add_field(ContactField::new(FieldType::Email, "first", "a@a.com"))
        .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "second",
        "+15551234567",
    ))
    .unwrap();
    card.add_field(ContactField::new(FieldType::Custom, "third", "val"))
        .unwrap();

    let id0 = card.fields()[0].id().to_string();
    let id1 = card.fields()[1].id().to_string();
    let id2 = card.fields()[2].id().to_string();

    // Reorder: third, first (second gets appended)
    card.reorder_fields(&[&id2, &id0]).unwrap();
    assert_eq!(card.fields()[0].id(), id2);
    assert_eq!(card.fields()[1].id(), id0);
    assert_eq!(card.fields()[2].id(), id1);
}

#[test]
fn test_reorder_fields_invalid_id() {
    let mut card = ContactCard::new("Test");
    card.add_field(ContactField::new(FieldType::Email, "a", "a@a.com"))
        .unwrap();
    let result = card.reorder_fields(&["nonexistent"]);
    result.expect_err("expected error");
}

// @scenario: contact_card_management:Add avatar to contact card
#[test]
fn test_set_avatar() {
    let mut card = ContactCard::new("Test");
    assert!(card.avatar().is_none());

    card.set_avatar(vec![0xFF, 0xD8, 0xFF]).unwrap();
    card.avatar().expect("expected Some");
    assert_eq!(card.avatar().unwrap(), &[0xFF, 0xD8, 0xFF]);
}

// @scenario: contact_card_management:Avatar image too large
#[test]
fn test_set_avatar_too_large() {
    let mut card = ContactCard::new("Test");
    let large = vec![0u8; 262145]; // MAX_AVATAR_SIZE + 1
    let result = card.set_avatar(large);
    result.expect_err("expected error");
}

// @scenario: contact_card_management:Add avatar to contact card
#[test]
fn test_set_avatar_at_max_size() {
    let mut card = ContactCard::new("Test");
    let max = vec![0u8; 262144]; // exactly MAX_AVATAR_SIZE
    card.set_avatar(max).unwrap();
    card.avatar().expect("expected Some");
}

// @scenario: contact_card_management:Remove avatar from contact card
#[test]
fn test_clear_avatar() {
    let mut card = ContactCard::new("Test");
    card.set_avatar(vec![1, 2, 3]).unwrap();
    card.avatar().expect("expected Some");

    card.clear_avatar();
    assert!(card.avatar().is_none());
}

#[test]
fn test_fields_mut() {
    let mut card = ContactCard::new("Test");
    card.add_field(ContactField::new(FieldType::Email, "work", "a@a.com"))
        .unwrap();

    let fields = card.fields_mut();
    assert_eq!(fields.len(), 1);
}

#[test]
fn test_card_id_unique() {
    let card1 = ContactCard::new("Test");
    let card2 = ContactCard::new("Test");
    assert_ne!(card1.id(), card2.id());
}

// @scenario: contact_card_management.feature:Field value size limit
#[test]
fn test_field_value_at_max_length() {
    let long_value = "x".repeat(1000 /* MAX_VALUE_LENGTH */);
    let field = ContactField::new(FieldType::Custom, "notes", &long_value);
    assert!(
        field.validate().is_ok(),
        "Value at MAX_VALUE_LENGTH should be valid"
    );
}

// @scenario: contact_card_management.feature:Field value size limit
#[test]
fn test_field_value_exceeds_max_length() {
    let too_long = "x".repeat(1000 /* MAX_VALUE_LENGTH */ + 1);
    let field = ContactField::new(FieldType::Custom, "notes", &too_long);
    let err = field
        .validate()
        .expect_err("should reject value > MAX_VALUE_LENGTH");
    assert!(
        format!("{err}").contains("too long") || format!("{err:?}").contains("ValueTooLong"),
        "Error should indicate value too long: {err:?}"
    );
}

// @scenario: contact_card_management.feature:Field value size limit
#[test]
fn test_field_label_at_max_length() {
    let long_label = "L".repeat(64 /* MAX_LABEL_LENGTH */);
    let field = ContactField::new(FieldType::Custom, &long_label, "value");
    // Label length validation is on the field, not separate
    assert!(
        field.validate().is_ok(),
        "Label at MAX_LABEL_LENGTH should be valid"
    );
}

// @scenario: contact_card_management.feature:Cancel editing preserves original values
#[test]
fn test_update_field_preserves_other_fields() {
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(FieldType::Email, "work", "old@work.com"))
        .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+41791234567",
    ))
    .unwrap();

    let email_id = card.fields()[0].id().to_string();

    // Update only the email field
    card.update_field_value(&email_id, "new@work.com").unwrap();

    // Email updated
    assert_eq!(card.fields()[0].value(), "new@work.com");
    // Phone preserved
    assert_eq!(card.fields()[1].value(), "+41791234567");
    assert_eq!(card.fields()[1].label(), "mobile");
}

// @scenario: contact_card_management.feature:Remove field updates contacts
#[test]
fn test_remove_field_preserves_remaining() {
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(FieldType::Email, "work", "a@b.com"))
        .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+41791234567",
    ))
    .unwrap();
    card.add_field(ContactField::new(FieldType::Custom, "notes", "friend"))
        .unwrap();

    let phone_id = card.fields()[1].id().to_string();

    // Remove the middle field
    card.remove_field(&phone_id).unwrap();

    // Should have 2 fields remaining
    assert_eq!(card.fields().len(), 2);
    assert_eq!(card.fields()[0].label(), "work");
    assert_eq!(card.fields()[1].label(), "notes");
}

// @scenario: contact_card_management.feature:Cancel field removal
#[test]
fn test_remove_nonexistent_field_fails() {
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(FieldType::Email, "work", "a@b.com"))
        .unwrap();

    let result = card.remove_field("nonexistent-id");
    assert!(result.is_err(), "Removing nonexistent field should fail");

    // Original field preserved
    assert_eq!(card.fields().len(), 1);
    assert_eq!(card.fields()[0].label(), "work");
}
