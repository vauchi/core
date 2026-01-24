//! Tests for field validation (social::validation)
//!
//! Tests crowd-sourced validation of all field types:
//! - Social profiles (twitter, github, etc.)
//! - Email addresses
//! - Phone numbers
//! - Websites
//! - Addresses
//! - Custom fields

use std::collections::HashSet;
use vauchi_core::social::*;
use vauchi_core::*;

// === Trust Level Tests ===

#[test]
fn test_trust_level_from_count() {
    assert_eq!(TrustLevel::from_count(0), TrustLevel::Unverified);
    assert_eq!(TrustLevel::from_count(1), TrustLevel::LowConfidence);
    assert_eq!(TrustLevel::from_count(2), TrustLevel::PartialConfidence);
    assert_eq!(TrustLevel::from_count(4), TrustLevel::PartialConfidence);
    assert_eq!(TrustLevel::from_count(5), TrustLevel::HighConfidence);
    assert_eq!(TrustLevel::from_count(100), TrustLevel::HighConfidence);
}

#[test]
fn test_trust_level_labels() {
    assert_eq!(TrustLevel::Unverified.label(), "unverified");
    assert_eq!(TrustLevel::LowConfidence.label(), "low confidence");
    assert_eq!(TrustLevel::PartialConfidence.label(), "partial confidence");
    assert_eq!(TrustLevel::HighConfidence.label(), "verified");
}

#[test]
fn test_trust_level_colors() {
    assert_eq!(TrustLevel::Unverified.color(), "grey");
    assert_eq!(TrustLevel::LowConfidence.color(), "yellow");
    assert_eq!(TrustLevel::PartialConfidence.color(), "light_green");
    assert_eq!(TrustLevel::HighConfidence.color(), "green");
}

// === Validation Status Tests ===

#[test]
fn test_validation_status_new() {
    let status = ValidationStatus::new("@alice");

    assert_eq!(status.count, 0);
    assert_eq!(status.trust_level, TrustLevel::Unverified);
    assert!(!status.validated_by_me);
    assert_eq!(status.field_value, "@alice");
}

#[test]
fn test_validation_status_display_no_validations() {
    let status = ValidationStatus::new("@alice");
    let names = std::collections::HashMap::new();

    assert_eq!(status.display(&names), "Not verified");
}

#[test]
fn test_validation_status_display_with_known_names() {
    let mut status = ValidationStatus::new("@alice");
    status.count = 3;
    status.validator_ids = vec!["bob".into(), "carol".into(), "dave".into()];

    let mut names = std::collections::HashMap::new();
    names.insert("bob".to_string(), "Bob".to_string());

    assert_eq!(status.display(&names), "Verified by Bob and 2 others");
}

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

#[test]
fn test_email_validation_trust_levels() {
    // 3 validations should give partial confidence
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
    assert_eq!(status.trust_level, TrustLevel::PartialConfidence);
}

// === Phone Validation Tests ===

#[test]
fn test_validate_phone_field() {
    let validator = Identity::create("Validator");

    let validation =
        ProfileValidation::create_signed(&validator, "mobile", "+1-555-123-4567", "bob_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_name(), Some("mobile"));
    assert_eq!(validation.field_value(), "+1-555-123-4567");
}

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
    assert_eq!(phone_status.trust_level, TrustLevel::HighConfidence);
    assert_eq!(email_status.count, 2);
    assert_eq!(email_status.trust_level, TrustLevel::PartialConfidence);
}

// === Website Validation Tests ===

#[test]
fn test_validate_website_field() {
    let validator = Identity::create("Validator");

    let validation =
        ProfileValidation::create_signed(&validator, "blog", "https://bob.dev", "bob_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_name(), Some("blog"));
    assert_eq!(validation.field_value(), "https://bob.dev");
}

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

#[test]
fn test_validate_custom_field() {
    let validator = Identity::create("Validator");

    let validation =
        ProfileValidation::create_signed(&validator, "signal", "bob.42", "bob_contact_id");

    assert!(validation.verify(validator.signing_public_key()));
    assert_eq!(validation.field_name(), Some("signal"));
    assert_eq!(validation.field_value(), "bob.42");
}

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
    assert_eq!(status_old.trust_level, TrustLevel::HighConfidence);

    // New value has 0 validations (the old validations don't count)
    let status_new =
        ValidationStatus::from_validations(&validations, "@bob_new", None, &HashSet::new());
    assert_eq!(status_new.count, 0);
    assert_eq!(status_new.trust_level, TrustLevel::Unverified);
}

// === From Stored Tests ===

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
    assert_eq!(status.trust_level, TrustLevel::HighConfidence);
}

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
