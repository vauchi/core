// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact_card::field
//! Extracted from field.rs

use vauchi_core::contact_card::*;

// @scenario: contact_card_management :: Add field to contact card
#[test]
fn test_create_field() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-1234");
    assert_eq!(field.field_type(), FieldType::Phone);
    assert_eq!(field.label(), "Mobile");
    assert_eq!(field.value(), "+1-555-1234");
}

// @scenario: field_validation :: Valid phone number formats
#[test]
fn test_validate_valid_phone() {
    let field = ContactField::new(FieldType::Phone, "Test", "+1-555-123-4567");
    field.validate().expect("expected success");
}

// @scenario: field_validation :: Valid email address formats
#[test]
fn test_validate_valid_email() {
    let field = ContactField::new(FieldType::Email, "Test", "test@example.com");
    field.validate().expect("expected success");
}

// @scenario: unicode_normalization :: Field label NFC normalization
#[test]
fn test_field_label_normalized_nfc() {
    let field = ContactField::new(FieldType::Phone, "Te\u{0301}le\u{0301}phone", "+41");
    assert_eq!(field.label(), "T\u{00E9}l\u{00E9}phone");
}

// @scenario: unicode_normalization :: Field value NFC normalization
#[test]
fn test_field_value_normalized_nfc() {
    let mut field = ContactField::new(FieldType::Custom, "Note", "cafe\u{0301}");
    assert_eq!(field.value(), "caf\u{00E9}");
    field.set_value("n\u{0303}");
    assert_eq!(field.value(), "\u{00F1}");
}
