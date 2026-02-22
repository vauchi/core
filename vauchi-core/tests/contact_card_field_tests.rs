// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact_card::field
//! Extracted from field.rs

use vauchi_core::contact_card::*;

// @scenario: contact_card_management.feature:Add field to contact card
#[test]
fn test_create_field() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-1234");
    assert_eq!(field.field_type(), FieldType::Phone);
    assert_eq!(field.label(), "Mobile");
    assert_eq!(field.value(), "+1-555-1234");
}

// @scenario: field_validation.feature:Valid phone number formats
#[test]
fn test_validate_valid_phone() {
    let field = ContactField::new(FieldType::Phone, "Test", "+1-555-123-4567");
    assert!(field.validate().is_ok());
}

// @scenario: field_validation.feature:Valid email address formats
#[test]
fn test_validate_valid_email() {
    let field = ContactField::new(FieldType::Email, "Test", "test@example.com");
    assert!(field.validate().is_ok());
}
