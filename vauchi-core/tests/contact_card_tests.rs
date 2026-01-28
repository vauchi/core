// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact_card
//! Extracted from mod.rs

use vauchi_core::*;

#[test]
fn test_create_card() {
    let card = ContactCard::new("Test User");
    assert_eq!(card.display_name(), "Test User");
    assert!(card.fields().is_empty());
}

#[test]
fn test_add_and_remove_field() {
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Email, "Work", "test@test.com");
    card.add_field(field).unwrap();
    assert_eq!(card.fields().len(), 1);

    let field_id = card.fields()[0].id().to_string();
    card.remove_field(&field_id).unwrap();
    assert!(card.fields().is_empty());
}
