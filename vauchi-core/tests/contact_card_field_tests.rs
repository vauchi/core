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

// --- note field tests ---

#[test]
fn test_field_note_default_none() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41 79 123 45 67");
    assert_eq!(f.note(), None);
}

#[test]
fn test_field_with_note() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41 79 123 45 67")
        .with_note("check spam".to_string());
    assert_eq!(f.note(), Some("check spam"));
}

#[test]
fn test_field_note_truncated_at_500_chars() {
    let long_note = "x".repeat(600);
    let f = ContactField::new(FieldType::Phone, "Work", "+41...").with_note(long_note);
    assert_eq!(f.note().unwrap().chars().count(), 500);
}

#[test]
fn test_field_empty_note_is_none() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41...").with_note("".to_string());
    assert_eq!(f.note(), None);
}

#[test]
fn test_strip_private_removes_note() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41 79 123 45 67")
        .with_note("secret".to_string());
    let stripped = f.strip_private();
    assert_eq!(stripped.note(), None);
    assert_eq!(stripped.value(), f.value());
    assert_eq!(stripped.label(), f.label());
    assert_eq!(stripped.id(), f.id());
}

#[test]
fn test_strip_private_on_field_without_note() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41...");
    let stripped = f.strip_private();
    assert_eq!(stripped.note(), None);
    assert_eq!(stripped.value(), f.value());
}

#[test]
fn test_note_serde_roundtrip() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41...").with_note("my note".to_string());
    let json = serde_json::to_string(&f).unwrap();
    let restored: ContactField = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.note(), Some("my note"));
}

#[test]
fn test_note_backward_compat_deserialize() {
    // Old JSON without note field should deserialize fine
    let json =
        r#"{"id":"abc","field_type":"Phone","label":"Work","value":"+41...","updated_at":0}"#;
    let f: ContactField = serde_json::from_str(json).unwrap();
    assert_eq!(f.note(), None);
}

#[test]
fn test_field_note_truncated_multibyte_utf8() {
    // 600 CJK characters (3 bytes each = 1800 bytes) should truncate to 500 characters
    let cjk_note: String = "\u{4e16}".repeat(600); // 世
    assert_eq!(cjk_note.chars().count(), 600);
    let f = ContactField::new(FieldType::Custom, "Note", "val").with_note(cjk_note);
    let note = f.note().unwrap();
    assert_eq!(note.chars().count(), 500);
    // All characters should be the same CJK character
    assert!(note.chars().all(|c| c == '\u{4e16}'));
}
