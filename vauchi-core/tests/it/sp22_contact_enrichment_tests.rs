// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! SP-22 Contact Enrichment: Phases 2 and 3
//! Feature: contact_enrichment.feature @annotations @birthday
//!
//! Phase 2: Local Annotations — nickname field
//! Phase 3: Birthday Field — FieldType::Birthday with ISO 8601 validation

use vauchi_core::contact_card::{ContactCard, FieldType};

// @internal
#[test]
fn test_contact_card_nickname_field_add_and_retrieve() {
    // RED: ContactCard doesn't have nickname() method yet
    let mut card = ContactCard::new("Alice");

    // Should be able to set nickname
    card.set_nickname("Al");

    // Should be able to get nickname
    assert_eq!(card.nickname(), Some("Al"));
}

// @internal
#[test]
fn test_contact_card_nickname_field_empty_clears() {
    // RED: ContactCard doesn't support clearing nickname
    let mut card = ContactCard::new("Alice");
    card.set_nickname("Al");

    // Setting empty string should clear it
    card.set_nickname("");

    assert_eq!(card.nickname(), None);
}

// @internal
#[test]
fn test_contact_card_nickname_field_max_length() {
    // RED: Nickname should respect max length constraint
    let mut card = ContactCard::new("Alice");

    // Max length should be 100 chars (or similar)
    let long_nickname = "a".repeat(101);
    card.set_nickname(&long_nickname);

    // Should truncate to max length
    let nickname = card.nickname().unwrap();
    assert!(nickname.len() <= 100);
}

// @internal
#[test]
fn test_birthday_field_type_exists() {
    let field_type = FieldType::Birthday;
    assert_eq!(format!("{field_type:?}"), "Birthday");
}

// @internal
#[test]
fn test_birthday_field_iso8601_valid_date() {
    // RED: Can't create Birthday field with ISO 8601 date yet
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "1995-03-15");
    field.validate().expect("Valid ISO 8601 date should pass");
}

// @internal
#[test]
fn test_birthday_field_iso8601_invalid_date_format() {
    // RED: Birthday validation should reject non-ISO 8601 formats
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "03/15/1995");

    // Should reject non-ISO 8601 format
    field.validate().expect_err("expected error");
}

// @internal
#[test]
fn test_birthday_field_iso8601_invalid_date_value() {
    // RED: Birthday validation should reject impossible dates
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "2025-13-45");

    // Should reject invalid date components
    field.validate().expect_err("expected error");
}

// @internal
#[test]
fn test_contact_card_single_birthday_constraint() {
    // RED: ContactCard should enforce single birthday
    let mut card = ContactCard::new("Alice");

    // Add first birthday
    use vauchi_core::contact_card::ContactField;
    let birthday1 = ContactField::new(FieldType::Birthday, "Birthday", "1995-03-15");
    card.add_field(birthday1)
        .expect("Should add first birthday");

    // Try to add second birthday — should fail
    let birthday2 = ContactField::new(FieldType::Birthday, "Birthday", "1990-01-01");
    let result = card.add_field(birthday2);

    // Should reject second birthday
    result.expect_err("expected error");
    assert_eq!(card.fields().len(), 1);
}

// @internal
#[test]
fn test_birthday_field_leap_year_valid() {
    // RED: Birthday should validate leap year dates
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "2000-02-29");
    field.validate().expect("Leap year date should be valid");
}

// @internal
#[test]
fn test_birthday_field_leap_year_invalid() {
    // RED: Birthday should reject invalid leap year dates
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "1900-02-29");

    // 1900 is not a leap year
    field.validate().expect_err("expected error");
}
