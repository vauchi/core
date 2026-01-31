// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Additional ContactCard tests for coverage of set_display_name, update_field,
//! remove_field, validate_size, reorder_fields, avatar methods.

use vauchi_core::{ContactCard, ContactField, FieldType};

#[test]
fn test_set_display_name() {
    let mut card = ContactCard::new("Original");
    card.set_display_name("Updated").unwrap();
    assert_eq!(card.display_name(), "Updated");
}

#[test]
fn test_set_display_name_empty_fails() {
    let mut card = ContactCard::new("Original");
    let result = card.set_display_name("");
    assert!(result.is_err());
}

#[test]
fn test_set_display_name_too_long_fails() {
    let mut card = ContactCard::new("Original");
    let long = "X".repeat(101);
    let result = card.set_display_name(&long);
    assert!(result.is_err());
}

#[test]
fn test_set_display_name_max_length() {
    let mut card = ContactCard::new("Original");
    let exactly_100 = "X".repeat(100);
    card.set_display_name(&exactly_100).unwrap();
    assert_eq!(card.display_name(), exactly_100);
}

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
    assert!(result.is_err());
}

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
    assert!(result.is_err());
}

#[test]
fn test_remove_field_not_found() {
    let mut card = ContactCard::new("Test");
    let result = card.remove_field("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_max_fields_reached() {
    let mut card = ContactCard::new("Test");
    for i in 0..25 {
        card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field_{}", i),
            &format!("value_{}", i),
        ))
        .unwrap();
    }
    let result = card.add_field(ContactField::new(FieldType::Custom, "extra", "value"));
    assert!(result.is_err());
}

#[test]
fn test_validate_size_ok() {
    let card = ContactCard::new("Test");
    card.validate_size().unwrap();
}

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
    assert!(result.is_err());
}

#[test]
fn test_set_avatar() {
    let mut card = ContactCard::new("Test");
    assert!(card.avatar().is_none());

    card.set_avatar(vec![0xFF, 0xD8, 0xFF]).unwrap();
    assert!(card.avatar().is_some());
    assert_eq!(card.avatar().unwrap(), &[0xFF, 0xD8, 0xFF]);
}

#[test]
fn test_set_avatar_too_large() {
    let mut card = ContactCard::new("Test");
    let large = vec![0u8; 262145]; // MAX_AVATAR_SIZE + 1
    let result = card.set_avatar(large);
    assert!(result.is_err());
}

#[test]
fn test_set_avatar_at_max_size() {
    let mut card = ContactCard::new("Test");
    let max = vec![0u8; 262144]; // exactly MAX_AVATAR_SIZE
    card.set_avatar(max).unwrap();
    assert!(card.avatar().is_some());
}

#[test]
fn test_clear_avatar() {
    let mut card = ContactCard::new("Test");
    card.set_avatar(vec![1, 2, 3]).unwrap();
    assert!(card.avatar().is_some());

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
