// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Additional ContactCard tests for coverage of set_display_name, update_field,
//! remove_field, validate_size, reorder_fields, avatar methods.

use image::{ImageBuffer, Rgba, RgbaImage};
use std::io::Cursor;
use vauchi_core::{ContactCard, ContactField, FieldType};

/// Helper: create a small valid PNG for avatar tests.
fn test_avatar_png() -> Vec<u8> {
    let img: RgbaImage = ImageBuffer::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

// @scenario: contact_card_management :: Update display name
// @internal
#[test]
fn test_set_display_name() {
    let mut card = ContactCard::new("Original");
    card.set_display_name("Updated").unwrap();
    assert_eq!(card.display_name(), "Updated");
}

// @scenario: contact_card_management :: Display name cannot be empty
// @internal
#[test]
fn test_set_display_name_empty_fails() {
    let mut card = ContactCard::new("Original");
    let result = card.set_display_name("");
    result.expect_err("expected error");
}

// @scenario: contact_card_management :: Display name length limit
// @internal
#[test]
fn test_set_display_name_too_long_fails() {
    let mut card = ContactCard::new("Original");
    let long = "X".repeat(101);
    let result = card.set_display_name(&long);
    result.expect_err("expected error");
}

// @scenario: contact_card_management :: Display name length limit
// @internal
#[test]
fn test_set_display_name_max_length() {
    let mut card = ContactCard::new("Original");
    let exactly_100 = "X".repeat(100);
    card.set_display_name(&exactly_100).unwrap();
    assert_eq!(card.display_name(), exactly_100);
}

// @scenario: contact_card_management :: Edit an existing field value
// @internal
#[test]
fn test_update_field_value() {
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Email, "work", "old@test.com", 0);
    card.add_field(field).unwrap();

    let field_id = card.fields()[0].id().to_string();
    card.update_field_value(&field_id, "new@test.com").unwrap();
    assert_eq!(card.fields()[0].value(), "new@test.com");
}

// @internal
#[test]
fn test_update_field_value_not_found() {
    let mut card = ContactCard::new("Test");
    let result = card.update_field_value("nonexistent", "value");
    result.expect_err("expected error");
}

// @scenario: contact_card_management :: Edit a field label
// @internal
#[test]
fn test_update_field_label() {
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Email, "work", "test@test.com", 0);
    card.add_field(field).unwrap();

    let field_id = card.fields()[0].id().to_string();
    card.update_field_label(&field_id, "personal").unwrap();
    assert_eq!(card.fields()[0].label(), "personal");
}

// @internal
#[test]
fn test_update_field_label_not_found() {
    let mut card = ContactCard::new("Test");
    let result = card.update_field_label("nonexistent", "label");
    result.expect_err("expected error");
}

// @internal
#[test]
fn test_remove_field_not_found() {
    let mut card = ContactCard::new("Test");
    let result = card.remove_field("nonexistent");
    result.expect_err("expected error");
}

// @scenario: contact_card_management :: Exceed maximum fields
// @scenario: contact_card_management :: Maximum number of fields
// @internal
#[test]
fn test_max_fields_reached() {
    let mut card = ContactCard::new("Test");
    for i in 0..vauchi_core::contact_card::MAX_FIELDS {
        card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field_{}", i),
            &format!("value_{}", i),
            0,
        ))
        .unwrap();
    }
    let result = card.add_field(ContactField::new(FieldType::Custom, "extra", "value", 0));
    result.expect_err("expected error");
}

// @scenario: contact_card_management :: Contact card size limit
// @internal
#[test]
fn test_validate_size_ok() {
    let card = ContactCard::new("Test");
    assert!(
        card.validate_size().is_ok(),
        "Default card should pass size validation"
    );
}

// @scenario: contact_card_management :: Reorder contact fields
// @internal
#[test]
fn test_reorder_fields() {
    let mut card = ContactCard::new("Test");
    card.add_field(ContactField::new(FieldType::Email, "first", "a@a.com", 0))
        .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "second",
        "+15551234567",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(FieldType::Custom, "third", "val", 0))
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

// @internal
#[test]
fn test_reorder_fields_invalid_id() {
    let mut card = ContactCard::new("Test");
    card.add_field(ContactField::new(FieldType::Email, "a", "a@a.com", 0))
        .unwrap();
    let result = card.reorder_fields(&["nonexistent"]);
    result.expect_err("expected error");
}

// @scenario: contact_card_management :: Add avatar to contact card
// @internal
#[test]
fn test_set_avatar() {
    let mut card = ContactCard::new("Test");
    assert!(card.avatar().is_none());

    card.set_avatar(test_avatar_png()).unwrap();
    let avatar = card.avatar().expect("expected Some");
    // set_avatar normalizes to WebP (ADR-042)
    assert_eq!(&avatar[0..4], b"RIFF");
    assert_eq!(&avatar[8..12], b"WEBP");
}

// @scenario: contact_card_management :: Avatar invalid format rejected
// @internal
#[test]
fn test_set_avatar_invalid_format() {
    let mut card = ContactCard::new("Test");
    let garbage = vec![0x00, 0x01, 0x02];
    let result = card.set_avatar(garbage);
    result.expect_err("expected error for invalid image format");
}

// @scenario: contact_card_management :: Large avatar image normalized
// @internal
#[test]
fn test_set_avatar_large_image_normalized() {
    let mut card = ContactCard::new("Test");
    let img: RgbaImage = ImageBuffer::from_fn(800, 800, |x, y| {
        Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
    });
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    card.set_avatar(buf.into_inner()).unwrap();
    let avatar = card.avatar().expect("expected Some");
    assert!(
        avatar.len() <= 32_768,
        "avatar should be <= 32 KB after normalization"
    );
    assert_eq!(&avatar[0..4], b"RIFF");
}

// @scenario: contact_card_management :: Remove avatar from contact card
// @internal
#[test]
fn test_clear_avatar() {
    let mut card = ContactCard::new("Test");
    card.set_avatar(test_avatar_png()).unwrap();
    card.avatar().expect("expected Some");

    card.clear_avatar();
    assert!(card.avatar().is_none());
}

// @internal
#[test]
fn test_fields_mut() {
    let mut card = ContactCard::new("Test");
    card.add_field(ContactField::new(FieldType::Email, "work", "a@a.com", 0))
        .unwrap();

    let fields = card.fields_mut();
    assert_eq!(fields.len(), 1);
}

// @internal
#[test]
fn test_card_id_unique() {
    let card1 = ContactCard::new("Test");
    let card2 = ContactCard::new("Test");
    assert_ne!(card1.id(), card2.id());
}
