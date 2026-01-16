//! TDD Tests for Contact Card Management
//!
//! These tests map directly to scenarios in contact_card_management.feature
//! Written FIRST (RED phase) before implementation.

use webbook_core::contact_card::{ContactCard, ContactField, FieldType};

// =============================================================================
// Contact Card Creation Tests
// =============================================================================

/// Tests that a contact card can be created with a display name
#[test]
fn test_create_contact_card() {
    let card = ContactCard::new("Alice Smith");

    assert_eq!(card.display_name(), "Alice Smith");
    assert!(card.fields().is_empty());
}

/// Tests that contact card has a unique ID
#[test]
fn test_contact_card_has_id() {
    let card = ContactCard::new("Alice");

    assert!(!card.id().is_empty());
}

// =============================================================================
// Adding Contact Fields Tests (from contact_card_management.feature)
// Scenario: Add a phone number field
// =============================================================================

/// Tests adding a phone number field
/// Maps to: "Add a phone number field"
#[test]
fn test_add_phone_field() {
    let mut card = ContactCard::new("Alice");

    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    card.add_field(field).expect("Adding field should succeed");

    assert_eq!(card.fields().len(), 1);
    let phone = &card.fields()[0];
    assert_eq!(phone.field_type(), FieldType::Phone);
    assert_eq!(phone.label(), "Mobile");
    assert_eq!(phone.value(), "+1-555-123-4567");
}

/// Tests adding an email field
/// Maps to: "Add an email field"
#[test]
fn test_add_email_field() {
    let mut card = ContactCard::new("Alice");

    let field = ContactField::new(FieldType::Email, "Work", "alice@company.com");
    card.add_field(field).expect("Adding field should succeed");

    let email = &card.fields()[0];
    assert_eq!(email.field_type(), FieldType::Email);
    assert_eq!(email.value(), "alice@company.com");
}

/// Tests adding social media fields
/// Maps to: "Add social media fields"
/// Note: Social networks use generic Social type with label identifying the network
#[test]
fn test_add_social_fields() {
    let mut card = ContactCard::new("Alice");

    // Social networks are identified by their label, not a specific type variant
    card.add_field(ContactField::new(
        FieldType::Social,
        "Twitter",
        "@alicesmith",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Social,
        "Instagram",
        "alice.smith",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Social,
        "LinkedIn",
        "linkedin.com/in/as",
    ))
    .unwrap();
    card.add_field(ContactField::new(FieldType::Social, "GitHub", "alicesmith"))
        .unwrap();

    assert_eq!(card.fields().len(), 4);

    // Verify they all have Social type with different labels
    for field in card.fields() {
        assert_eq!(field.field_type(), FieldType::Social);
    }
}

/// Tests adding an address field
/// Maps to: "Add a physical address field"
#[test]
fn test_add_address_field() {
    let mut card = ContactCard::new("Alice");

    let field = ContactField::new(FieldType::Address, "Home", "123 Main St, City, ST 12345");
    card.add_field(field).expect("Adding field should succeed");

    let address = &card.fields()[0];
    assert_eq!(address.field_type(), FieldType::Address);
}

/// Tests adding a custom field
/// Maps to: "Add a custom field"
#[test]
fn test_add_custom_field() {
    let mut card = ContactCard::new("Alice");

    let field = ContactField::new(FieldType::Custom, "Signal", "+1-555-987-6543");
    card.add_field(field).expect("Adding field should succeed");

    let custom = &card.fields()[0];
    assert_eq!(custom.field_type(), FieldType::Custom);
    assert_eq!(custom.label(), "Signal");
}

// =============================================================================
// Field Validation Tests (from contact_card_management.feature)
// =============================================================================

/// Tests that valid phone numbers are accepted
#[test]
fn test_validate_phone_valid_formats() {
    let valid_phones = [
        "+1-555-123-4567",
        "555-123-4567",
        "+44 20 7946 0958",
        "(555) 123-4567",
    ];

    for phone in valid_phones {
        let field = ContactField::new(FieldType::Phone, "Test", phone);
        assert!(
            field.validate().is_ok(),
            "Phone '{}' should be valid",
            phone
        );
    }
}

/// Tests that invalid phone numbers are rejected
#[test]
fn test_validate_phone_invalid_formats() {
    let invalid_phones = ["not-a-phone", "abc"];

    for phone in invalid_phones {
        let field = ContactField::new(FieldType::Phone, "Test", phone);
        assert!(
            field.validate().is_err(),
            "Phone '{}' should be invalid",
            phone
        );
    }
}

/// Tests that valid emails are accepted
#[test]
fn test_validate_email_valid_formats() {
    let valid_emails = [
        "alice@example.com",
        "alice+tag@example.com",
        "alice@sub.example.com",
    ];

    for email in valid_emails {
        let field = ContactField::new(FieldType::Email, "Test", email);
        assert!(
            field.validate().is_ok(),
            "Email '{}' should be valid",
            email
        );
    }
}

/// Tests that invalid emails are rejected
#[test]
fn test_validate_email_invalid_formats() {
    let invalid_emails = ["invalid-email", "@example.com", "alice@"];

    for email in invalid_emails {
        let field = ContactField::new(FieldType::Email, "Test", email);
        assert!(
            field.validate().is_err(),
            "Email '{}' should be invalid",
            email
        );
    }
}

/// Tests that field values exceeding max length are rejected
/// Maps to: "Field value size limit"
#[test]
fn test_field_value_max_length() {
    let long_value = "x".repeat(1001); // Exceeds 1000 char limit

    let field = ContactField::new(FieldType::Custom, "Test", &long_value);
    assert!(
        field.validate().is_err(),
        "Value exceeding 1000 chars should be invalid"
    );
}

// =============================================================================
// Editing Contact Fields Tests
// =============================================================================

/// Tests editing a field value
/// Maps to: "Edit an existing field value"
#[test]
fn test_edit_field_value() {
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1-555-123-4567",
    ))
    .unwrap();

    let field_id = card.fields()[0].id().to_string();
    card.update_field_value(&field_id, "+1-555-999-8888")
        .expect("Update should succeed");

    assert_eq!(card.fields()[0].value(), "+1-555-999-8888");
}

/// Tests editing a field label
/// Maps to: "Edit a field label"
#[test]
fn test_edit_field_label() {
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "alice@work.com",
    ))
    .unwrap();

    let field_id = card.fields()[0].id().to_string();
    card.update_field_label(&field_id, "Office")
        .expect("Update should succeed");

    assert_eq!(card.fields()[0].label(), "Office");
}

// =============================================================================
// Removing Contact Fields Tests
// =============================================================================

/// Tests removing a field
/// Maps to: "Remove a field from contact card"
#[test]
fn test_remove_field() {
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1-555-123-4567",
    ))
    .unwrap();

    let field_id = card.fields()[0].id().to_string();
    card.remove_field(&field_id).expect("Remove should succeed");

    assert!(card.fields().is_empty());
}

// =============================================================================
// Display Name Tests
// =============================================================================

/// Tests updating display name
/// Maps to: "Update display name"
#[test]
fn test_update_display_name() {
    let mut card = ContactCard::new("Alice Smith");

    card.set_display_name("Alice S.")
        .expect("Update should succeed");

    assert_eq!(card.display_name(), "Alice S.");
}

/// Tests that empty display name is rejected
/// Maps to: "Display name cannot be empty"
#[test]
fn test_empty_display_name_rejected() {
    let mut card = ContactCard::new("Alice");

    let result = card.set_display_name("");

    assert!(result.is_err());
}

/// Tests display name length limit
/// Maps to: "Display name length limit"
#[test]
fn test_display_name_length_limit() {
    let mut card = ContactCard::new("Alice");
    let long_name = "x".repeat(101); // Exceeds 100 char limit

    let result = card.set_display_name(&long_name);

    assert!(result.is_err());
}

// =============================================================================
// Contact Card Limits Tests
// =============================================================================

/// Tests maximum number of fields (25)
/// Maps to: "Maximum number of fields"
#[test]
fn test_card_max_fields_limit() {
    let mut card = ContactCard::new("Alice");

    // Add 25 fields (the max)
    for i in 0..25 {
        let field = ContactField::new(FieldType::Custom, &format!("Field{}", i), "value");
        card.add_field(field)
            .expect(&format!("Adding field {} should succeed", i));
    }

    assert_eq!(card.fields().len(), 25);

    // Try to add one more - should fail
    let field = ContactField::new(FieldType::Custom, "TooMany", "value");
    let result = card.add_field(field);

    assert!(result.is_err(), "Adding 26th field should fail");
}

// =============================================================================
// Field Type Tests
// =============================================================================

/// Tests field type serialization
#[test]
fn test_field_type_variants() {
    let types = [
        FieldType::Phone,
        FieldType::Email,
        FieldType::Social,
        FieldType::Address,
        FieldType::Website,
        FieldType::Custom,
    ];

    for field_type in types {
        let field = ContactField::new(field_type.clone(), "Test", "value");
        assert_eq!(field.field_type(), field_type);
    }
}
