//! Social Validation Integration Tests
//!
//! Tests for social profile validation and trust building features.
//! Covers the social_profile_validation.feature scenarios.

use std::collections::{HashMap, HashSet};
use webbook_core::{
    identity::Identity,
    social::{
        ProfileValidation, SocialNetwork, SocialNetworkRegistry, TrustLevel, ValidationStatus,
    },
};

// =============================================================================
// Profile Validation Tests
// =============================================================================

/// Test: Validation record creation and accessors
#[test]
fn test_validation_record_creation() {
    let signature = [0xABu8; 64];

    let validation = ProfileValidation::new("field-123", "@alice", "validator-id", signature);

    // Verify accessors
    assert_eq!(validation.field_id(), "field-123");
    assert_eq!(validation.field_value(), "@alice");
    assert_eq!(validation.validator_id(), "validator-id");
    assert!(validation.validated_at() > 0);
    assert_eq!(*validation.signature(), signature);

    // Signable bytes format
    let signable = validation.signable_bytes();
    assert!(String::from_utf8_lossy(&signable).contains("WEBBOOK_VALIDATION"));
    assert!(String::from_utf8_lossy(&signable).contains("field-123"));
    assert!(String::from_utf8_lossy(&signable).contains("@alice"));
}

/// Test: Validation verify with wrong signature fails
#[test]
fn test_validation_verify_wrong_signature() {
    let validator = Identity::create("Validator");
    let other = Identity::create("Other");

    // Create a validation with a random signature
    let validation = ProfileValidation::new(
        "field-123",
        "@alice",
        "validator-id",
        [0xABu8; 64], // Random bytes, not a valid signature
    );

    // Should fail verification with any key since signature is invalid
    assert!(
        !validation.verify(validator.signing_public_key()),
        "Invalid signature should fail verification"
    );
    assert!(
        !validation.verify(other.signing_public_key()),
        "Invalid signature should fail verification with any key"
    );
}

/// Test: Trust levels progress with validation count
#[test]
fn test_trust_level_progression() {
    assert_eq!(TrustLevel::from_count(0), TrustLevel::Unverified);
    assert_eq!(TrustLevel::from_count(1), TrustLevel::LowConfidence);
    assert_eq!(TrustLevel::from_count(2), TrustLevel::PartialConfidence);
    assert_eq!(TrustLevel::from_count(3), TrustLevel::PartialConfidence);
    assert_eq!(TrustLevel::from_count(4), TrustLevel::PartialConfidence);
    assert_eq!(TrustLevel::from_count(5), TrustLevel::HighConfidence);
    assert_eq!(TrustLevel::from_count(10), TrustLevel::HighConfidence);
    assert_eq!(TrustLevel::from_count(100), TrustLevel::HighConfidence);
}

/// Test: Validation status filters blocked validators
#[test]
fn test_validation_status_blocks_validators() {
    let validations = vec![
        ProfileValidation::new("field1", "@alice", "trusted1", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "blocked1", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "trusted2", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "blocked2", [0u8; 64]),
    ];

    let mut blocked = HashSet::new();
    blocked.insert("blocked1".to_string());
    blocked.insert("blocked2".to_string());

    let status = ValidationStatus::from_validations(&validations, "@alice", None, &blocked);

    assert_eq!(status.count, 2, "Should only count non-blocked validators");
    assert_eq!(status.trust_level, TrustLevel::PartialConfidence);
    assert!(!status.validator_ids.contains(&"blocked1".to_string()));
    assert!(!status.validator_ids.contains(&"blocked2".to_string()));
}

/// Test: Validation status invalidated on value change
#[test]
fn test_validation_invalidated_on_value_change() {
    let validations = vec![
        ProfileValidation::new("field1", "@alice_old", "bob", [0u8; 64]),
        ProfileValidation::new("field1", "@alice_old", "carol", [0u8; 64]),
    ];

    // Status for old value
    let status_old =
        ValidationStatus::from_validations(&validations, "@alice_old", None, &HashSet::new());
    assert_eq!(status_old.count, 2);

    // Status for new value - validations don't count
    let status_new = ValidationStatus::from_validations(
        &validations,
        "@alice_new", // Changed
        None,
        &HashSet::new(),
    );
    assert_eq!(
        status_new.count, 0,
        "Old validations shouldn't count for new value"
    );
    assert_eq!(status_new.trust_level, TrustLevel::Unverified);
}

/// Test: Validation display with known names
#[test]
fn test_validation_display_formatting() {
    let mut status = ValidationStatus::new("@alice");
    status.count = 3;
    status.validator_ids = vec!["bob".into(), "carol".into(), "unknown".into()];

    let mut names = HashMap::new();
    names.insert("bob".to_string(), "Bob".to_string());
    names.insert("carol".to_string(), "Carol".to_string());

    let display = status.display(&names);
    assert!(display.contains("Bob"), "Should include known name Bob");
    assert!(
        display.contains("Carol") || display.contains("1 other"),
        "Should include Carol or count"
    );
}

/// Test: Validation status tracks if I validated
#[test]
fn test_validation_tracks_self_validation() {
    let validations = vec![
        ProfileValidation::new("field1", "@alice", "bob", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "me", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "carol", [0u8; 64]),
    ];

    let status_with_me =
        ValidationStatus::from_validations(&validations, "@alice", Some("me"), &HashSet::new());
    assert!(status_with_me.validated_by_me);

    let status_without_me = ValidationStatus::from_validations(
        &validations,
        "@alice",
        Some("someone_else"),
        &HashSet::new(),
    );
    assert!(!status_without_me.validated_by_me);
}

// =============================================================================
// Social Network Registry Tests
// =============================================================================

/// Test: Registry contains major social networks
#[test]
fn test_registry_has_major_networks() {
    let registry = SocialNetworkRegistry::with_defaults();

    // Check major networks exist
    let major_networks = [
        "twitter",
        "github",
        "linkedin",
        "instagram",
        "facebook",
        "youtube",
        "tiktok",
    ];

    for network in major_networks {
        assert!(
            registry.get(network).is_some(),
            "Registry should contain {}",
            network
        );
    }
}

/// Test: Profile URL generation for various networks
#[test]
fn test_profile_url_generation() {
    let registry = SocialNetworkRegistry::with_defaults();

    // Twitter
    assert_eq!(
        registry.profile_url("twitter", "elonmusk"),
        Some("https://twitter.com/elonmusk".to_string())
    );

    // GitHub
    assert_eq!(
        registry.profile_url("github", "torvalds"),
        Some("https://github.com/torvalds".to_string())
    );

    // LinkedIn (uses /in/ path)
    assert_eq!(
        registry.profile_url("linkedin", "satyanadella"),
        Some("https://linkedin.com/in/satyanadella".to_string())
    );
}

/// Test: Handle @ symbol stripping
#[test]
fn test_at_symbol_handling() {
    let registry = SocialNetworkRegistry::with_defaults();

    // Twitter should strip @
    assert_eq!(
        registry.profile_url("twitter", "@username"),
        Some("https://twitter.com/username".to_string())
    );

    // Instagram should strip @
    assert_eq!(
        registry.profile_url("instagram", "@username"),
        Some("https://instagram.com/username".to_string())
    );
}

/// Test: Search finds networks by name
#[test]
fn test_registry_search() {
    let registry = SocialNetworkRegistry::with_defaults();

    let git_results = registry.search("git");
    assert!(git_results.len() >= 2, "Should find GitHub and GitLab");
    assert!(git_results.iter().any(|n| n.id() == "github"));
    assert!(git_results.iter().any(|n| n.id() == "gitlab"));

    let book_results = registry.search("book");
    assert!(book_results.iter().any(|n| n.id() == "facebook"));
}

/// Test: Custom network can be added
#[test]
fn test_custom_network_addition() {
    let mut registry = SocialNetworkRegistry::new();

    registry.add(SocialNetwork::new(
        "mynetwork",
        "My Network",
        "https://mynetwork.com/user/{username}",
    ));

    assert!(registry.get("mynetwork").is_some());
    assert_eq!(
        registry.profile_url("mynetwork", "alice"),
        Some("https://mynetwork.com/user/alice".to_string())
    );
}

/// Test: Registry merging
#[test]
fn test_registry_merge() {
    let mut registry1 = SocialNetworkRegistry::new();
    registry1.add(SocialNetwork::new(
        "net1",
        "Net 1",
        "https://net1.com/{username}",
    ));

    let mut registry2 = SocialNetworkRegistry::new();
    registry2.add(SocialNetwork::new(
        "net2",
        "Net 2",
        "https://net2.com/{username}",
    ));
    registry2.add(SocialNetwork::new(
        "net1",
        "Net 1 Updated",
        "https://updated.com/{username}",
    ));

    registry1.merge(&registry2);

    assert_eq!(registry1.len(), 2);
    assert_eq!(
        registry1.get("net1").unwrap().display_name(),
        "Net 1 Updated"
    );
}

/// Test: Registry serialization roundtrip
#[test]
fn test_registry_serialization() {
    let registry = SocialNetworkRegistry::with_defaults();

    let json = registry.to_json().unwrap();
    let restored = SocialNetworkRegistry::from_json(&json).unwrap();

    assert_eq!(registry.len(), restored.len());

    // Verify a sample network survived
    let twitter = restored.get("twitter").unwrap();
    assert!(twitter.display_name().contains("Twitter"));
}

// =============================================================================
// Trust Level Edge Cases
// =============================================================================

/// Test: Trust level labels are consistent
#[test]
fn test_trust_level_labels() {
    assert_eq!(TrustLevel::Unverified.label(), "unverified");
    assert_eq!(TrustLevel::LowConfidence.label(), "low confidence");
    assert_eq!(TrustLevel::PartialConfidence.label(), "partial confidence");
    assert_eq!(TrustLevel::HighConfidence.label(), "verified");
}

/// Test: Trust level colors are valid
#[test]
fn test_trust_level_colors() {
    let colors = [
        TrustLevel::Unverified.color(),
        TrustLevel::LowConfidence.color(),
        TrustLevel::PartialConfidence.color(),
        TrustLevel::HighConfidence.color(),
    ];

    for color in colors {
        assert!(!color.is_empty(), "Color should not be empty");
    }
}

/// Test: Validation with no validators
#[test]
fn test_empty_validation_status() {
    let status = ValidationStatus::from_validations(&[], "@alice", None, &HashSet::new());

    assert_eq!(status.count, 0);
    assert_eq!(status.trust_level, TrustLevel::Unverified);
    assert!(!status.validated_by_me);
    assert!(status.validator_ids.is_empty());
}

/// Test: Display for various counts
#[test]
fn test_display_various_counts() {
    let names = HashMap::new();

    let mut status = ValidationStatus::new("@test");

    status.count = 0;
    assert_eq!(status.display(&names), "Not verified");

    status.count = 1;
    status.validator_ids = vec!["v1".into()];
    let display = status.display(&names);
    assert!(display.contains("1") && display.contains("person"));

    status.count = 5;
    status.validator_ids = vec![
        "v1".into(),
        "v2".into(),
        "v3".into(),
        "v4".into(),
        "v5".into(),
    ];
    let display = status.display(&names);
    assert!(display.contains("5") && display.contains("people"));
}
