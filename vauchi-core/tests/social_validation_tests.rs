//! Tests for social::validation
//! Extracted from validation.rs

use std::collections::HashSet;
use vauchi_core::social::*;
use vauchi_core::*;

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

#[test]
fn test_create_signed_validation() {
    // Create an identity for the validator
    let validator = Identity::create("Validator");

    // Create a signed validation
    let validation =
        ProfileValidation::create_signed(&validator, "twitter", "@alice", "alice_contact_id");

    // Verify the signature is valid
    assert!(validation.verify(validator.signing_public_key()));

    // Check field parsing
    assert_eq!(validation.contact_id(), Some("alice_contact_id"));
    assert_eq!(validation.field_name(), Some("twitter"));
    assert_eq!(validation.field_value(), "@alice");
}

#[test]
fn test_validation_signature_prevents_tampering() {
    let validator = Identity::create("Validator");
    let attacker = Identity::create("Attacker");

    // Create a valid validation
    let validation =
        ProfileValidation::create_signed(&validator, "twitter", "@alice", "alice_contact_id");

    // Signature should verify with validator's key
    assert!(validation.verify(validator.signing_public_key()));

    // Signature should NOT verify with attacker's key
    assert!(!validation.verify(attacker.signing_public_key()));
}
