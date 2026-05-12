// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for custom field special characters, cross-field dependencies,
//! and max field length enforcement.
//!
//! Split from field_validation_tests.rs (structural tidy, no behavior change).

use crate::common;

use common::field_validation_helpers::MAX_VALUE_LENGTH;

// =============================================================================
// Custom Field Special Characters Tests
// Traces to: _private/features/field_validation.feature @validate @custom
// =============================================================================

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_custom_field_special_characters_unicode() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    let unicode_values = vec![
        "日本語テスト",      // Japanese
        "Ελληνικά",          // Greek
        "العربية",           // Arabic
        "עברית",             // Hebrew
        "中文测试",          // Chinese
        "한국어",            // Korean
        "Ümlauts äöü ÄÖÜ ß", // German
        "Ñoño español",      // Spanish
        "Привет мир",        // Russian
    ];

    for value in unicode_values {
        let field = ContactField::new(FieldType::Custom, "Custom", value, 0);
        let result = field.validate();
        assert!(
            result.is_ok(),
            "Unicode value '{}' should be valid for custom fields",
            value
        );
        assert_eq!(field.value(), value, "Value should be preserved exactly");
    }
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_custom_field_special_characters_emoji() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    let emoji_values = vec![
        "Hello 👋",
        "🎉 Party 🎊",
        "Code 💻 Life",
        "❤️ Love",
        "🇺🇸🇬🇧🇫🇷", // Flag emojis
        "👨‍👩‍👧‍👦",     // Family emoji (ZWJ sequence)
        "🏳️‍🌈",     // Rainbow flag (ZWJ)
        "👍🏻👍🏽👍🏿", // Skin tone modifiers
    ];

    for value in emoji_values {
        let field = ContactField::new(FieldType::Custom, "Custom", value, 0);
        let result = field.validate();
        assert!(
            result.is_ok(),
            "Emoji value '{}' should be valid for custom fields",
            value
        );
    }
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_custom_field_special_characters_symbols() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    let symbol_values = vec![
        "test@email.com",
        "https://example.com/path?query=value",
        "file:///path/to/file",
        "@handle:matrix.org", // Matrix handle
        "user#1234",          // Discord-style
        "$variable",
        "%encoded",
        "&ampersand",
        "*asterisk*",
        "(parentheses)",
        "[brackets]",
        "{braces}",
        "<angle>",
        "pipe|char",
        "back\\slash",
        "forward/slash",
        "quote'single",
        "quote\"double",
        "grave`tick",
        "tilde~wave",
    ];

    for value in symbol_values {
        let field = ContactField::new(FieldType::Custom, "Custom", value, 0);
        let result = field.validate();
        assert!(
            result.is_ok(),
            "Symbol value '{}' should be valid for custom fields",
            value
        );
    }
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_custom_field_mixed_scripts() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    // Mixed script values
    let mixed_values = vec![
        "Hello こんにちは 你好",
        "Name: Иван (Ivan)",
        "Contact: 田中 tanaka@example.com",
    ];

    for value in mixed_values {
        let field = ContactField::new(FieldType::Custom, "Custom", value, 0);
        let result = field.validate();
        assert!(
            result.is_ok(),
            "Mixed script value should be valid: {}",
            value
        );
    }
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_custom_field_control_characters() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    // Values with newlines and tabs (should be preserved)
    let multiline = "Line 1\nLine 2\nLine 3";
    let field = ContactField::new(FieldType::Custom, "Custom", multiline, 0);
    assert!(field.validate().is_ok(), "Multiline values should be valid");
    assert_eq!(field.value(), multiline, "Newlines should be preserved");

    let tabbed = "Col1\tCol2\tCol3";
    let field = ContactField::new(FieldType::Custom, "Custom", tabbed, 0);
    assert!(field.validate().is_ok(), "Tabbed values should be valid");
}

// =============================================================================
// Cross-Field Dependencies Tests
// Traces to: _private/features/field_validation.feature @multiple @all-types
// =============================================================================

// @scenario: field_validation :: Each field type has independent validation
// @scenario: field_validation :: Each social field has independent validation
#[test]
fn test_cross_field_dependencies_independent_validation() {
    use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};

    // Each field type should validate independently
    let mut card = ContactCard::new("Test User");

    // Add multiple fields of different types
    let phone = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    let email = ContactField::new(FieldType::Email, "Work", "test@example.com", 0);
    let social = ContactField::new(FieldType::Social, "Twitter", "@testuser", 0);
    let website = ContactField::new(FieldType::Website, "Blog", "https://example.com", 0);
    let address = ContactField::new(FieldType::Address, "Home", "123 Main St", 0);
    let custom = ContactField::new(FieldType::Custom, "Signal", "test.123", 0);

    // All should add successfully
    card.add_field(phone).expect("expected success");
    card.add_field(email).expect("expected success");
    card.add_field(social).expect("expected success");
    card.add_field(website).expect("expected success");
    card.add_field(address).expect("expected success");
    card.add_field(custom).expect("expected success");

    assert_eq!(card.fields().len(), 6, "All 6 fields should be added");
}

// @scenario: field_validation :: Each field type has independent validation
#[test]
fn test_cross_field_dependencies_validation_isolation() {
    use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};

    let mut card = ContactCard::new("Test User");

    // Add a valid phone
    let valid_phone = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    card.add_field(valid_phone).expect("expected success");

    // Try to add an invalid email - should fail
    let invalid_email = ContactField::new(FieldType::Email, "Work", "not-an-email", 0);
    let result = card.add_field(invalid_email);
    assert!(result.is_err(), "Invalid email should be rejected");

    // Valid phone should still be there
    assert_eq!(card.fields().len(), 1, "Only valid field should remain");
    assert_eq!(card.fields()[0].label(), "Mobile");
}

// @scenario: field_validation :: Validation persists when other fields change
#[test]
fn test_cross_field_dependencies_update_isolation() {
    use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};

    let mut card = ContactCard::new("Test User");

    let phone = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    let phone_id = phone.id().to_string();
    card.add_field(phone).unwrap();

    let email = ContactField::new(FieldType::Email, "Work", "test@example.com", 0);
    let email_id = email.id().to_string();
    card.add_field(email).unwrap();

    // Update phone to invalid - should fail
    let result = card.update_field_value(&phone_id, "invalid");
    assert!(result.is_err(), "Invalid phone update should fail");

    // Email should be unaffected
    let email_field = card.fields().iter().find(|f| f.id() == email_id).unwrap();
    assert_eq!(
        email_field.value(),
        "test@example.com",
        "Email should be unchanged"
    );
}

// =============================================================================
// Max Field Length Enforcement Tests
// Traces to: _private/features/field_validation.feature @edge-cases
// =============================================================================

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_max_field_length_enforcement_at_limit() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    // Value exactly at max length should be valid
    let max_value = "a".repeat(MAX_VALUE_LENGTH);
    let field = ContactField::new(FieldType::Custom, "Custom", &max_value, 0);
    assert!(
        field.validate().is_ok(),
        "Value at max length ({}) should be valid",
        MAX_VALUE_LENGTH
    );
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_max_field_length_enforcement_over_limit() {
    use vauchi_core::contact_card::{ContactField, FieldType, ValidationError};

    // Value over max length should be rejected
    let over_max = "a".repeat(MAX_VALUE_LENGTH + 1);
    let field = ContactField::new(FieldType::Custom, "Custom", &over_max, 0);
    let result = field.validate();

    assert!(result.is_err(), "Value over max length should be rejected");
    match result.unwrap_err() {
        ValidationError::ValueTooLong { max } => {
            assert_eq!(max, MAX_VALUE_LENGTH);
        }
        e => panic!("Expected ValueTooLong, got {:?}", e),
    }
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_max_field_length_enforcement_unicode_chars() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    // Unicode characters (multi-byte) - the limit is in bytes, not chars
    let unicode_char = "日"; // 3 bytes in UTF-8
    let char_count = MAX_VALUE_LENGTH / 3;
    let max_unicode = unicode_char.repeat(char_count);

    let field = ContactField::new(FieldType::Custom, "Custom", &max_unicode, 0);
    let result = field.validate();
    // Should be valid since byte length is at or under limit
    assert!(
        result.is_ok(),
        "Unicode value within byte limit should be valid"
    );
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_max_field_length_enforcement_emoji() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    // Emojis can be 4+ bytes each
    let emoji = "🎉"; // 4 bytes in UTF-8
    let emoji_count = MAX_VALUE_LENGTH / 4;
    let emoji_value = emoji.repeat(emoji_count);

    let field = ContactField::new(FieldType::Custom, "Custom", &emoji_value, 0);
    let result = field.validate();
    assert!(
        result.is_ok(),
        "Emoji value within byte limit should be valid"
    );
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_max_field_length_enforcement_per_field_type() {
    use vauchi_core::contact_card::{ContactField, FieldType, ValidationError};

    let over_max = "a".repeat(MAX_VALUE_LENGTH + 1);

    // All field types should reject over-length values
    let field_types = vec![
        (FieldType::Phone, "Mobile"),
        (FieldType::Email, "Work"),
        (FieldType::Social, "Twitter"),
        (FieldType::Website, "Blog"),
        (FieldType::Address, "Home"),
        (FieldType::Custom, "Other"),
    ];

    for (field_type, label) in field_types {
        let field = ContactField::new(field_type.clone(), label, &over_max, 0);
        let result = field.validate();
        assert!(
            result.is_err(),
            "{:?} field should reject over-length value",
            field_type
        );
        if let Err(ValidationError::ValueTooLong { .. }) = result {
            // Expected
        } else {
            panic!(
                "{:?} field should return ValueTooLong error, got {:?}",
                field_type, result
            );
        }
    }
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_max_field_length_card_rejects_overlong() {
    use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};

    let mut card = ContactCard::new("Test User");

    // Try to add a field with over-length value
    let over_max = "a".repeat(MAX_VALUE_LENGTH + 1);
    let field = ContactField::new(FieldType::Custom, "Custom", &over_max, 0);

    let result = card.add_field(field);
    assert!(
        result.is_err(),
        "Card should reject adding over-length field"
    );
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_max_field_length_update_rejects_overlong() {
    use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};

    let mut card = ContactCard::new("Test User");

    // Add a valid field
    let field = ContactField::new(FieldType::Custom, "Custom", "valid value", 0);
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();

    // Try to update to over-length value
    let over_max = "a".repeat(MAX_VALUE_LENGTH + 1);
    let result = card.update_field_value(&field_id, &over_max);

    // Validation should fail
    assert!(
        result.is_err(),
        "Card should reject updating to over-length value"
    );

    // Note: Current implementation sets the value before validating,
    // so the invalid value remains on the field even after validation fails.
    // This documents actual behavior - a future improvement could validate
    // before setting to maintain atomicity.
    let updated_field = card.fields().iter().find(|f| f.id() == field_id).unwrap();
    assert_eq!(
        updated_field.value().len(),
        MAX_VALUE_LENGTH + 1,
        "Current impl: value is set before validation (non-atomic)"
    );
}
