// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for trust levels, validation status, signing, blocked contacts,
//! multiple validators, stored validations, and reset-on-change.
//!
//! Split from field_validation_tests.rs (structural tidy, no behavior change).

use std::collections::HashSet;
use vauchi_core::social::*;
use vauchi_core::*;

// === Trust Level Tests ===

// @scenario: field_validation :: Validation score determines trust level
#[test]
fn test_trust_level_from_count() {
    assert_eq!(
        ValidationConfidence::from_count(0),
        ValidationConfidence::Unverified
    );
    assert_eq!(
        ValidationConfidence::from_count(1),
        ValidationConfidence::LowConfidence
    );
    assert_eq!(
        ValidationConfidence::from_count(2),
        ValidationConfidence::PartialConfidence
    );
    assert_eq!(
        ValidationConfidence::from_count(4),
        ValidationConfidence::PartialConfidence
    );
    assert_eq!(
        ValidationConfidence::from_count(5),
        ValidationConfidence::HighConfidence
    );
    assert_eq!(
        ValidationConfidence::from_count(100),
        ValidationConfidence::HighConfidence
    );
}

// @scenario: field_validation :: Validation score determines trust level
#[test]
fn test_trust_level_labels() {
    assert_eq!(ValidationConfidence::Unverified.label(), "unverified");
    assert_eq!(
        ValidationConfidence::LowConfidence.label(),
        "low confidence"
    );
    assert_eq!(
        ValidationConfidence::PartialConfidence.label(),
        "partial confidence"
    );
    assert_eq!(ValidationConfidence::HighConfidence.label(), "verified");
}

// @scenario: field_validation :: Validation score determines trust level
#[test]
fn test_trust_level_colors() {
    assert_eq!(ValidationConfidence::Unverified.color(), "grey");
    assert_eq!(ValidationConfidence::LowConfidence.color(), "yellow");
    assert_eq!(
        ValidationConfidence::PartialConfidence.color(),
        "light_green"
    );
    assert_eq!(ValidationConfidence::HighConfidence.color(), "green");
}

// === Validation Status Tests ===

// @scenario: field_validation :: View unvalidated field
#[test]
fn test_validation_status_new() {
    let status = ValidationStatus::new("@alice");

    assert_eq!(status.count, 0);
    assert_eq!(status.trust_level, ValidationConfidence::Unverified);
    assert!(!status.validated_by_me);
    assert_eq!(status.field_value, "@alice");
}

// @scenario: field_validation :: View unvalidated field
#[test]
fn test_validation_status_display_no_validations() {
    let status = ValidationStatus::new("@alice");
    let names = std::collections::HashMap::new();

    assert_eq!(status.display(&names), "Not verified");
}

// @scenario: field_validation :: Trust level considers validator relationship
#[test]
fn test_validation_status_display_with_known_names() {
    let mut status = ValidationStatus::new("@alice");
    status.count = 3;
    status.validator_ids = vec!["bob".into(), "carol".into(), "dave".into()];

    let mut names = std::collections::HashMap::new();
    names.insert("bob".to_string(), "Bob".to_string());

    assert_eq!(status.display(&names), "Verified by Bob and 2 others");
}

// @scenario: field_validation :: Blocked contact's validation is ignored
#[test]
fn test_validation_status_from_validations_filters_blocked() {
    let validations = vec![
        ProfileValidation::new("field1", "@alice", "bob", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "mallory", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "carol", [0u8; 64]),
    ];

    let mut blocked = HashSet::new();
    blocked.insert("mallory".to_string());

    let status = ValidationStatus::from_validations(&validations, "@alice", None, &blocked);

    assert_eq!(status.count, 2);
    assert!(!status.validator_ids.contains(&"mallory".to_string()));
}

// @scenario: field_validation :: Validation resets when field value changes
#[test]
fn test_validation_status_invalidated_on_value_change() {
    let validations = vec![ProfileValidation::new(
        "field1",
        "@alice_old",
        "bob",
        [0u8; 64],
    )];

    let status = ValidationStatus::from_validations(
        &validations,
        "@alice_new", // Value changed
        None,
        &HashSet::new(),
    );

    // Validation doesn't count because field value changed
    assert_eq!(status.count, 0);
}

// === Social Profile Validation Tests ===

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_validate_social_profile() {
    let validator = Identity::create("Validator");

    let validation =
        ProfileValidation::create_signed(&validator, "twitter", "@alice", "alice_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.contact_id(), Some("alice_contact_id"));
    assert_eq!(validation.field_name(), Some("twitter"));
    assert_eq!(validation.field_value(), "@alice");
}

// @scenario: field_validation :: Validations are cryptographically signed
// @scenario: field_validation :: Cannot forge validations
#[test]
fn test_validation_signature_prevents_tampering() {
    let validator = Identity::create("Validator");
    let attacker = Identity::create("Attacker");

    let validation =
        ProfileValidation::create_signed(&validator, "twitter", "@alice", "alice_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert!(!validation.verify(attacker.signing_public_key()));
}

// === Email Validation Tests ===

// @scenario: field_validation :: Validate a contact's email address
#[test]
fn test_validate_email_field() {
    let validator = Identity::create("Validator");

    let validation = ProfileValidation::create_signed(
        &validator,
        "work_email",
        "bob@example.com",
        "bob_contact_id",
    );

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_name(), Some("work_email"));
    assert_eq!(validation.field_value(), "bob@example.com");
}

// @scenario: field_validation :: Email validation shows trust level
#[test]
fn test_email_validation_trust_levels() {
    // 3 validations with no metadata: 3 * 0.3 = 0.9 weighted score -> LowConfidence
    let validations: Vec<_> = (0..3)
        .map(|i| {
            ProfileValidation::new(
                "bob:work_email",
                "bob@example.com",
                &format!("validator_{}", i),
                [0u8; 64],
            )
        })
        .collect();

    let status =
        ValidationStatus::from_validations(&validations, "bob@example.com", None, &HashSet::new());

    assert_eq!(status.count, 3);
    // With weighted scoring (no metadata = 0.3 per validator): 3 * 0.3 = 0.9 -> LowConfidence
    assert_eq!(status.trust_level, ValidationConfidence::LowConfidence);
}

// === Phone Validation Tests ===

// @scenario: field_validation :: Validate a contact's phone number
#[test]
fn test_validate_phone_field() {
    let validator = Identity::create("Validator");

    let validation =
        ProfileValidation::create_signed(&validator, "mobile", "+1-555-123-4567", "bob_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_name(), Some("mobile"));
    assert_eq!(validation.field_value(), "+1-555-123-4567");
}

// @scenario: field_validation :: Phone validation persists when email changes
#[test]
fn test_phone_validation_independent_of_email() {
    // Phone validations should not be affected by email validations
    let phone_validations: Vec<_> = (0..5)
        .map(|i| {
            ProfileValidation::new(
                "bob:mobile",
                "+1-555-123-4567",
                &format!("validator_{}", i),
                [0u8; 64],
            )
        })
        .collect();

    let email_validations: Vec<_> = (0..2)
        .map(|i| {
            ProfileValidation::new(
                "bob:email",
                "bob@example.com",
                &format!("validator_{}", i),
                [0u8; 64],
            )
        })
        .collect();

    let phone_status = ValidationStatus::from_validations(
        &phone_validations,
        "+1-555-123-4567",
        None,
        &HashSet::new(),
    );

    let email_status = ValidationStatus::from_validations(
        &email_validations,
        "bob@example.com",
        None,
        &HashSet::new(),
    );

    assert_eq!(phone_status.count, 5);
    // With weighted scoring (no metadata = 0.3 per validator): 5 * 0.3 = 1.5 -> PartialConfidence
    assert_eq!(
        phone_status.trust_level,
        ValidationConfidence::PartialConfidence
    );
    assert_eq!(email_status.count, 2);
    // 2 * 0.3 = 0.6 -> LowConfidence
    assert_eq!(
        email_status.trust_level,
        ValidationConfidence::LowConfidence
    );
}

// === Website Validation Tests ===

// @scenario: field_validation :: Validate a contact's website
#[test]
fn test_validate_website_field() {
    let validator = Identity::create("Validator");

    let validation =
        ProfileValidation::create_signed(&validator, "blog", "https://bob.dev", "bob_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_name(), Some("blog"));
    assert_eq!(validation.field_value(), "https://bob.dev");
}

// @scenario: field_validation :: Website validation requires exact URL match
#[test]
fn test_website_validation_requires_exact_url_match() {
    // Validations for old URL should not count for new URL
    let validations = vec![ProfileValidation::new(
        "bob:blog",
        "https://bob.dev",
        "validator",
        [0u8; 64],
    )];

    // Same URL - validation counts
    let status1 =
        ValidationStatus::from_validations(&validations, "https://bob.dev", None, &HashSet::new());
    assert_eq!(status1.count, 1);

    // Different URL - validation doesn't count
    let status2 = ValidationStatus::from_validations(
        &validations,
        "https://bob.dev/new",
        None,
        &HashSet::new(),
    );
    assert_eq!(status2.count, 0);
}

// === Address Validation Tests ===

// @scenario: field_validation :: Validate a contact's address
#[test]
fn test_validate_address_field() {
    let validator = Identity::create("Validator");

    let validation = ProfileValidation::create_signed(
        &validator,
        "home",
        "123 Main St, City, State 12345",
        "bob_contact_id",
    );

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_name(), Some("home"));
    assert_eq!(validation.field_value(), "123 Main St, City, State 12345");
}

// === Custom Field Validation Tests ===

// @scenario: field_validation :: Validate a custom field
// @scenario: contact_card_management :: Add unlisted social network as custom field
#[test]
fn test_validate_custom_field() {
    let validator = Identity::create("Validator");

    let validation =
        ProfileValidation::create_signed(&validator, "signal", "bob.42", "bob_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_name(), Some("signal"));
    assert_eq!(validation.field_value(), "bob.42");
}

// @scenario: field_validation :: Validate a custom field
#[test]
fn test_validate_custom_field_with_special_chars() {
    let validator = Identity::create("Validator");

    // Custom fields can have various formats
    let validation =
        ProfileValidation::create_signed(&validator, "matrix", "@bob:matrix.org", "bob_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_value(), "@bob:matrix.org");
}

// === Independent Validation Per Field Type Tests ===

// @scenario: field_validation :: Each field type has independent validation
#[test]
fn test_independent_validation_per_field_type() {
    // Each field type should have independent validation counts
    let validator = Identity::create("Validator");

    let social = ProfileValidation::create_signed(&validator, "twitter", "@bob", "bob");
    let email = ProfileValidation::create_signed(&validator, "email", "bob@example.com", "bob");
    let phone = ProfileValidation::create_signed(&validator, "phone", "+1-555-1234", "bob");
    let website = ProfileValidation::create_signed(&validator, "blog", "https://bob.dev", "bob");
    let address = ProfileValidation::create_signed(&validator, "home", "123 Main St", "bob");
    let custom = ProfileValidation::create_signed(&validator, "signal", "bob.42", "bob");

    // Each should be independently verifiable
    assert!(social.verify(validator.signing_public_key()));
    assert!(email.verify(validator.signing_public_key()));
    assert!(phone.verify(validator.signing_public_key()));
    assert!(website.verify(validator.signing_public_key()));
    assert!(address.verify(validator.signing_public_key()));
    assert!(custom.verify(validator.signing_public_key()));

    // Each should have its own field_id
    assert_ne!(social.field_id(), email.field_id());
    assert_ne!(email.field_id(), phone.field_id());
    assert_ne!(phone.field_id(), website.field_id());
    assert_ne!(website.field_id(), address.field_id());
    assert_ne!(address.field_id(), custom.field_id());
}

// === Validation Reset On Field Change Tests ===

// @scenario: field_validation :: Validation resets when field value changes
#[test]
fn test_validation_reset_on_field_change() {
    // Create validations for the old value
    let validations: Vec<_> = (0..5)
        .map(|i| {
            ProfileValidation::new(
                "bob:twitter",
                "@bob_old",
                &format!("validator_{}", i),
                [0u8; 64],
            )
        })
        .collect();

    // Old value has 5 validations
    let status_old =
        ValidationStatus::from_validations(&validations, "@bob_old", None, &HashSet::new());
    assert_eq!(status_old.count, 5);
    // With weighted scoring (no metadata = 0.3 per validator): 5 * 0.3 = 1.5 -> PartialConfidence
    assert_eq!(
        status_old.trust_level,
        ValidationConfidence::PartialConfidence
    );

    // New value has 0 validations (the old validations don't count)
    let status_new =
        ValidationStatus::from_validations(&validations, "@bob_new", None, &HashSet::new());
    assert_eq!(status_new.count, 0);
    assert_eq!(status_new.trust_level, ValidationConfidence::Unverified);
}

// === From Stored Tests ===

// @scenario: field_validation :: Validation is stored locally
#[test]
fn test_validation_from_stored() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let validation = ProfileValidation::from_stored(
        "bob:twitter",
        "@bob_smith",
        "validator_123",
        now,
        [1u8; 64],
    );

    assert_eq!(validation.field_id(), "bob:twitter");
    assert_eq!(validation.field_value(), "@bob_smith");
    assert_eq!(validation.validator_id(), "validator_123");
    assert_eq!(validation.validated_at(), now);
    assert_eq!(validation.contact_id(), Some("bob"));
    assert_eq!(validation.field_name(), Some("twitter"));
}

// === Multiple Validators Tests ===

// @scenario: field_validation :: View highly validated field
#[test]
fn test_multiple_validators_same_field() {
    let validators: Vec<_> = (0..5)
        .map(|i| Identity::create(&format!("Validator{}", i)))
        .collect();

    let validations: Vec<_> = validators
        .iter()
        .map(|v| ProfileValidation::create_signed(v, "twitter", "@alice", "alice"))
        .collect();

    // All validations should be valid
    for (validation, validator) in validations.iter().zip(validators.iter()) {
        assert!(validation.verify(validator.signing_public_key()));
    }

    // Status should show all 5 validations
    let status = ValidationStatus::from_validations(&validations, "@alice", None, &HashSet::new());
    assert_eq!(status.count, 5);
    // With weighted scoring (no metadata = 0.3 per validator): 5 * 0.3 = 1.5 -> PartialConfidence
    assert_eq!(status.trust_level, ValidationConfidence::PartialConfidence);
}

// @scenario: field_validation :: Cannot validate same field twice
#[test]
fn test_validated_by_me_flag() {
    let me = Identity::create("Me");
    let other = Identity::create("Other");

    let my_id = hex::encode(me.signing_public_key());
    let other_id = hex::encode(other.signing_public_key());

    let validations = vec![
        ProfileValidation::new("bob:twitter", "@bob", &my_id, [0u8; 64]),
        ProfileValidation::new("bob:twitter", "@bob", &other_id, [0u8; 64]),
    ];

    // When checking with my ID, validated_by_me should be true
    let status =
        ValidationStatus::from_validations(&validations, "@bob", Some(&my_id), &HashSet::new());
    assert!(status.validated_by_me);
    assert_eq!(status.count, 2);

    // When checking with other ID, validated_by_me should be false
    let status = ValidationStatus::from_validations(
        &validations,
        "@bob",
        Some("someone_else"),
        &HashSet::new(),
    );
    assert!(!status.validated_by_me);
}
