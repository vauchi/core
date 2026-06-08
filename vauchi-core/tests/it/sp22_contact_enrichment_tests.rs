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

    card.set_nickname("Al");

    assert_eq!(card.nickname(), Some("Al"));
}

// @internal
#[test]
fn test_contact_card_nickname_field_empty_clears() {
    // RED: ContactCard doesn't support clearing nickname
    let mut card = ContactCard::new("Alice");
    card.set_nickname("Al");

    card.set_nickname("");

    assert_eq!(card.nickname(), None);
}

// @internal
#[test]
fn test_contact_card_nickname_field_max_length() {
    let mut card = ContactCard::new("Alice");

    // Max length should be 100 chars (or similar)
    let long_nickname = "a".repeat(101);
    card.set_nickname(&long_nickname);

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

    let field = ContactField::new(FieldType::Birthday, "Birthday", "1995-03-15", 0);
    field.validate().expect("Valid ISO 8601 date should pass");
}

// @internal
#[test]
fn test_birthday_field_iso8601_invalid_date_format() {
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "03/15/1995", 0);

    field.validate().expect_err("expected error");
}

// @internal
#[test]
fn test_birthday_field_iso8601_invalid_date_value() {
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "2025-13-45", 0);

    field.validate().expect_err("expected error");
}

// @internal
#[test]
fn test_contact_card_single_birthday_constraint() {
    let mut card = ContactCard::new("Alice");

    use vauchi_core::contact_card::ContactField;
    let birthday1 = ContactField::new(FieldType::Birthday, "Birthday", "1995-03-15", 0);
    card.add_field(birthday1)
        .expect("Should add first birthday");

    // Try to add second birthday — should fail
    let birthday2 = ContactField::new(FieldType::Birthday, "Birthday", "1990-01-01", 0);
    let result = card.add_field(birthday2);

    result.expect_err("expected error");
    assert_eq!(card.fields().len(), 1);
}

// @internal
#[test]
fn test_birthday_field_leap_year_valid() {
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "2000-02-29", 0);
    field.validate().expect("Leap year date should be valid");
}

// @internal
#[test]
fn test_birthday_field_leap_year_invalid() {
    use vauchi_core::contact_card::ContactField;

    let field = ContactField::new(FieldType::Birthday, "Birthday", "1900-02-29", 0);

    // 1900 is not a leap year
    field.validate().expect_err("expected error");
}
