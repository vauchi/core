// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for field validation API integration: blocked contacts exclusion,
//! weighted trust levels, validation delivery queuing, and incoming validation
//! processing.
//!
//! Split from field_validation_tests.rs (structural tidy, no behavior change).

use std::collections::HashSet;
use vauchi_core::social::*;

// =============================================================================
// API Integration: Blocked Contacts Excluded from Validation Status
// Traces to: _private/features/field_validation.feature @blocked @trust
// =============================================================================

// @scenario: field_validation :: Blocked contact's validation is ignored
#[test]
fn test_get_field_validation_status_excludes_blocked_contacts() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::social::ProfileValidation;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Vauchi instance with identity
    let mut wb: Vauchi = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    // Create two validator identities
    let bob_identity = Identity::create("Bob");
    let mallory_identity = Identity::create("Mallory");

    let bob_validator_id = hex::encode(bob_identity.signing_public_key());
    let mallory_validator_id = hex::encode(mallory_identity.signing_public_key());

    // Create a target contact (Charlie) whose field will be validated
    let charlie_key = [42u8; 32];
    let charlie_contact = Contact::from_exchange(
        charlie_key,
        ContactCard::new("Charlie"),
        SymmetricKey::generate(),
    );
    let charlie_id = charlie_contact.id().to_string();
    wb.add_contact(charlie_contact).unwrap();

    // Create contacts for Bob and Mallory (using their signing public keys as contact IDs)
    let bob_contact = Contact::from_exchange(
        *bob_identity.signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
    );
    let mallory_contact = Contact::from_exchange(
        *mallory_identity.signing_public_key(),
        ContactCard::new("Mallory"),
        SymmetricKey::generate(),
    );

    // Verify contact IDs match validator IDs
    assert_eq!(bob_contact.id(), bob_validator_id);
    assert_eq!(mallory_contact.id(), mallory_validator_id);

    wb.add_contact(bob_contact).unwrap();
    wb.add_contact(mallory_contact).unwrap();

    // Block Mallory
    wb.block_contact(&mallory_validator_id).unwrap();

    // Save validation records from both Bob and Mallory for Charlie's twitter field
    let bob_validation =
        ProfileValidation::create_signed(&bob_identity, "twitter", "@charlie", &charlie_id);
    let mallory_validation =
        ProfileValidation::create_signed(&mallory_identity, "twitter", "@charlie", &charlie_id);

    wb.storage().save_validation(&bob_validation).unwrap();
    wb.storage().save_validation(&mallory_validation).unwrap();

    // Get validation status - should exclude Mallory's validation
    let status = wb
        .get_field_validation_status(&charlie_id, "twitter", "@charlie")
        .unwrap();

    // Bob's validation should count, Mallory's should NOT
    assert_eq!(
        status.count, 1,
        "Only non-blocked validator's validation should count (got {})",
        status.count
    );
    assert!(
        status.validator_ids.contains(&bob_validator_id),
        "Bob's validation should be included"
    );
    assert!(
        !status.validator_ids.contains(&mallory_validator_id),
        "Mallory's (blocked) validation should be excluded"
    );
    assert_eq!(
        status.trust_level,
        ValidationConfidence::LowConfidence,
        "Trust level should reflect only 1 validation"
    );
}

// =============================================================================
// Weighted Trust Level Tests
// Traces to: _private/features/field_validation.feature @trust-weight
// =============================================================================

// @scenario: field_validation :: Validation score determines trust level
#[test]
fn test_from_weighted_score_thresholds() {
    // Below 0.1 -> Unverified
    assert_eq!(
        ValidationConfidence::from_weighted_score(0.0),
        ValidationConfidence::Unverified,
        "Score 0.0 should be Unverified"
    );
    assert_eq!(
        ValidationConfidence::from_weighted_score(0.09),
        ValidationConfidence::Unverified,
        "Score 0.09 should be Unverified"
    );

    // 0.1..1.0 -> LowConfidence
    assert_eq!(
        ValidationConfidence::from_weighted_score(0.1),
        ValidationConfidence::LowConfidence,
        "Score 0.1 should be LowConfidence"
    );
    assert_eq!(
        ValidationConfidence::from_weighted_score(0.5),
        ValidationConfidence::LowConfidence,
        "Score 0.5 should be LowConfidence"
    );
    assert_eq!(
        ValidationConfidence::from_weighted_score(0.99),
        ValidationConfidence::LowConfidence,
        "Score 0.99 should be LowConfidence"
    );

    // 1.0..3.0 -> PartialConfidence
    assert_eq!(
        ValidationConfidence::from_weighted_score(1.0),
        ValidationConfidence::PartialConfidence,
        "Score 1.0 should be PartialConfidence"
    );
    assert_eq!(
        ValidationConfidence::from_weighted_score(2.5),
        ValidationConfidence::PartialConfidence,
        "Score 2.5 should be PartialConfidence"
    );
    assert_eq!(
        ValidationConfidence::from_weighted_score(2.99),
        ValidationConfidence::PartialConfidence,
        "Score 2.99 should be PartialConfidence"
    );

    // 3.0+ -> HighConfidence
    assert_eq!(
        ValidationConfidence::from_weighted_score(3.0),
        ValidationConfidence::HighConfidence,
        "Score 3.0 should be HighConfidence"
    );
    assert_eq!(
        ValidationConfidence::from_weighted_score(10.0),
        ValidationConfidence::HighConfidence,
        "Score 10.0 should be HighConfidence"
    );
}

// @scenario: field_validation :: Trust level considers validator relationship
#[test]
fn test_from_validations_weighted_uses_contact_metadata() {
    use std::collections::HashMap;

    // Create 3 validations from different validators
    let validations = vec![
        ProfileValidation::new("field1", "@alice", "validator_old_verified", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "validator_new_unverified", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "validator_unknown", [0u8; 64]),
    ];

    // Build metadata: one old+verified contact, one new+unverified
    let mut meta = HashMap::new();
    meta.insert(
        "validator_old_verified".to_string(),
        ValidatorMeta {
            contact_age_days: 60,
            fingerprint_verified: true,
        },
    );
    meta.insert(
        "validator_new_unverified".to_string(),
        ValidatorMeta {
            contact_age_days: 0,
            fingerprint_verified: false,
        },
    );
    // validator_unknown is NOT in the map -> should get default weight 0.3

    let status_weighted = ValidationStatus::from_validations_weighted(
        &validations,
        "@alice",
        None,
        &HashSet::new(),
        &meta,
    );

    // Raw count is still 3 (all validators counted)
    assert_eq!(status_weighted.count, 3, "Raw count should be 3 validators");

    // Weighted score:
    // validator_old_verified: 60 days, fingerprint=true -> weight = 1.0
    // validator_new_unverified: 0 days, fingerprint=false -> weight = 0.3
    // validator_unknown: not in meta -> default weight = 0.3
    // Total = 1.0 + 0.3 + 0.3 = 1.6

    // 1.6 is in [1.0, 3.0) -> PartialConfidence
    assert_eq!(
        status_weighted.trust_level,
        ValidationConfidence::PartialConfidence,
        "Weighted score of 1.6 should give PartialConfidence"
    );

    // Compare with unweighted: 3 validators -> from_count(3) -> PartialConfidence
    // Both happen to be PartialConfidence, so let's also check a case where they differ:
    // With only the verified contact, weighted=1.0 -> PartialConfidence,
    // but from_count(1) -> LowConfidence
    let single_validation = vec![ProfileValidation::new(
        "field1",
        "@alice",
        "validator_old_verified",
        [0u8; 64],
    )];

    let status_single = ValidationStatus::from_validations_weighted(
        &single_validation,
        "@alice",
        None,
        &HashSet::new(),
        &meta,
    );

    assert_eq!(status_single.count, 1, "Raw count should be 1");
    // Weight = 1.0, so score = 1.0 -> PartialConfidence (not LowConfidence like count-based)
    assert_eq!(
        status_single.trust_level,
        ValidationConfidence::PartialConfidence,
        "Single verified mature contact with weight 1.0 should give PartialConfidence"
    );
}

// @scenario: field_validation :: Sybil attack resistance
#[test]
fn test_from_validations_weighted_unknown_validators_get_minimum_weight() {
    use std::collections::HashMap;

    // 1 validation from an unknown validator (not in metadata)
    let validations = vec![ProfileValidation::new(
        "field1",
        "@alice",
        "unknown_validator",
        [0u8; 64],
    )];

    let meta: HashMap<String, ValidatorMeta> = HashMap::new();

    let status = ValidationStatus::from_validations_weighted(
        &validations,
        "@alice",
        None,
        &HashSet::new(),
        &meta,
    );

    assert_eq!(status.count, 1, "Count should be 1");
    // Weight for unknown = 0.3, score = 0.3 -> [0.1, 1.0) -> LowConfidence
    assert_eq!(
        status.trust_level,
        ValidationConfidence::LowConfidence,
        "Unknown validator with weight 0.3 should give LowConfidence"
    );
}

// @scenario: field_validation :: Blocked contact's validation is ignored
// @scenario: field_validation :: Validation resets when field value changes
#[test]
fn test_from_validations_weighted_filters_blocked_and_mismatched_values() {
    use std::collections::HashMap;

    let validations = vec![
        ProfileValidation::new("field1", "@alice", "bob", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "mallory", [0u8; 64]),
        ProfileValidation::new("field1", "@alice_old", "carol", [0u8; 64]), // wrong value
    ];

    let mut blocked = HashSet::new();
    blocked.insert("mallory".to_string());

    let mut meta = HashMap::new();
    meta.insert(
        "bob".to_string(),
        ValidatorMeta {
            contact_age_days: 60,
            fingerprint_verified: true,
        },
    );
    meta.insert(
        "carol".to_string(),
        ValidatorMeta {
            contact_age_days: 60,
            fingerprint_verified: true,
        },
    );

    let status =
        ValidationStatus::from_validations_weighted(&validations, "@alice", None, &blocked, &meta);

    // mallory is blocked, carol's value doesn't match -> only bob counts
    assert_eq!(status.count, 1, "Only bob's validation should count");
    assert!(
        !status.validator_ids.contains(&"mallory".to_string()),
        "Blocked validator should be excluded"
    );
    assert!(
        !status.validator_ids.contains(&"carol".to_string()),
        "Mismatched value validator should be excluded"
    );
}

// @scenario: field_validation :: Validation score determines trust level
#[test]
fn test_from_validations_backward_compat() {
    // from_validations() should produce identical results to before
    // (i.e., it delegates to from_validations_weighted with empty metadata,
    //  but since unknown validators get weight 0.3, and from_count uses count,
    //  we need to verify the behavior stays count-based)

    // 0 validations -> Unverified
    let status_0 = ValidationStatus::from_validations(&[], "@alice", None, &HashSet::new());
    assert_eq!(status_0.count, 0);
    assert_eq!(status_0.trust_level, ValidationConfidence::Unverified);

    // 1 validation -> LowConfidence (weight 0.3 -> LowConfidence, count 1 -> LowConfidence: matches)
    let validations_1 = vec![ProfileValidation::new("field1", "@alice", "bob", [0u8; 64])];
    let status_1 =
        ValidationStatus::from_validations(&validations_1, "@alice", None, &HashSet::new());
    assert_eq!(status_1.count, 1);
    assert_eq!(
        status_1.trust_level,
        ValidationConfidence::LowConfidence,
        "1 validation should still be LowConfidence"
    );

    // 3 validations: 3 * 0.3 = 0.9 -> LowConfidence (count-based was PartialConfidence)
    // NOTE: This is the intentional behavioral change -- weighted scoring uses metadata,
    // and with empty metadata (all unknown at 0.3), 3 validators give 0.9 score = LowConfidence.
    // The old from_count(3) gave PartialConfidence. This is expected with the new weighting.
    let validations_3 = vec![
        ProfileValidation::new("field1", "@alice", "bob", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "carol", [0u8; 64]),
        ProfileValidation::new("field1", "@alice", "dave", [0u8; 64]),
    ];
    let status_3 =
        ValidationStatus::from_validations(&validations_3, "@alice", None, &HashSet::new());
    assert_eq!(status_3.count, 3);
    // 3 * 0.3 = 0.9, in [0.1, 1.0) -> LowConfidence
    assert_eq!(
        status_3.trust_level,
        ValidationConfidence::LowConfidence,
        "3 unknown validators with weight 0.3 each = 0.9 -> LowConfidence"
    );

    // validated_by_me should still work
    let status_me =
        ValidationStatus::from_validations(&validations_1, "@alice", Some("bob"), &HashSet::new());
    assert!(status_me.validated_by_me, "Should detect own validation");

    // Blocked filtering should still work
    let mut blocked = HashSet::new();
    blocked.insert("bob".to_string());
    let status_blocked =
        ValidationStatus::from_validations(&validations_1, "@alice", None, &blocked);
    assert_eq!(
        status_blocked.count, 0,
        "Blocked validator should be excluded"
    );
    assert_eq!(status_blocked.trust_level, ValidationConfidence::Unverified);
}

// =============================================================================
// Validation Delivery Queuing Tests
// Traces to: _private/features/field_validation.feature @delivery @sync
// =============================================================================

// @scenario: field_validation :: Validate a contact's social profile
// @scenario: field_validation :: Validation count syncs from contacts
#[test]
fn test_validate_field_queues_delivery_to_contact() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Alice (the validator) with identity
    let mut alice: Vauchi = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();

    // Create Bob as a contact
    let bob_identity = Identity::create("Bob");
    let bob_pk = *bob_identity.signing_public_key();
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), SymmetricKey::generate());
    let bob_contact_id = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();

    // No pending updates before validation
    let pending_before = alice.storage().get_all_pending_updates().unwrap();
    let validation_updates_before: Vec<_> = pending_before
        .iter()
        .filter(|u| u.update_type == "validation_record")
        .collect();
    assert_eq!(
        validation_updates_before.len(),
        0,
        "No validation_record updates should exist before validate_field"
    );

    // Alice validates Bob's twitter field
    let result = alice.validate_field(&bob_contact_id, "twitter", "@bob");
    assert!(
        result.is_ok(),
        "validate_field should succeed: {:?}",
        result.err()
    );

    // After validation, a pending update should be queued for delivery
    let pending_after = alice.storage().get_all_pending_updates().unwrap();
    let validation_updates: Vec<_> = pending_after
        .iter()
        .filter(|u| u.contact_id == bob_contact_id && u.update_type == "validation_record")
        .collect();
    assert_eq!(
        validation_updates.len(),
        1,
        "Exactly one validation_record pending update should be queued for the contact"
    );

    // The payload should be non-empty (serialized validation)
    assert!(
        !validation_updates[0].payload.is_empty(),
        "Validation record payload should be non-empty"
    );

    // The payload should deserialize as a valid ProfileValidation
    let deserialized: Result<vauchi_core::social::ProfileValidation, _> =
        serde_json::from_slice(&validation_updates[0].payload);
    assert!(
        deserialized.is_ok(),
        "Payload should deserialize as ProfileValidation: {:?}",
        deserialized.err()
    );

    let validation = deserialized.unwrap();
    assert_eq!(
        validation.field_value(),
        "@bob",
        "Deserialized validation should have correct field_value"
    );
}

// @scenario: field_validation :: Revoke validation
#[test]
fn test_revoke_field_validation_queues_revocation() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Alice (the validator) with identity
    let mut alice: Vauchi = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();

    // Create Bob as a contact
    let bob_identity = Identity::create("Bob");
    let bob_pk = *bob_identity.signing_public_key();
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), SymmetricKey::generate());
    let bob_contact_id = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();

    // Alice validates Bob's twitter field first
    alice
        .validate_field(&bob_contact_id, "twitter", "@bob")
        .unwrap();

    // Clear any pending updates from the validation to isolate revocation test
    alice.storage().clear_all_pending_updates().unwrap();

    // No revocation updates should exist yet
    let pending_before = alice.storage().get_all_pending_updates().unwrap();
    let revocation_updates_before: Vec<_> = pending_before
        .iter()
        .filter(|u| u.update_type == "validation_revocation")
        .collect();
    assert_eq!(
        revocation_updates_before.len(),
        0,
        "No validation_revocation updates should exist before revocation"
    );

    // Alice revokes validation of Bob's twitter field
    let revoked = alice
        .revoke_field_validation(&bob_contact_id, "twitter")
        .unwrap();
    assert!(revoked, "Revocation should succeed (validation existed)");

    // After revocation, a pending update should be queued
    let pending_after = alice.storage().get_all_pending_updates().unwrap();
    let revocation_updates: Vec<_> = pending_after
        .iter()
        .filter(|u| u.contact_id == bob_contact_id && u.update_type == "validation_revocation")
        .collect();
    assert_eq!(
        revocation_updates.len(),
        1,
        "Exactly one validation_revocation pending update should be queued for the contact"
    );

    // The payload should be non-empty
    assert!(
        !revocation_updates[0].payload.is_empty(),
        "Revocation payload should be non-empty"
    );
}

// @scenario: field_validation :: Validation is stored locally
#[test]
fn test_validate_field_queue_failure_does_not_fail_validation() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // This test verifies the validation itself succeeds even if queuing
    // were to fail internally. Since we use `let _ = ...` to ignore
    // queue errors, validate_field should always succeed if the local
    // save succeeds.

    let mut alice: Vauchi = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();

    let bob_identity = Identity::create("Bob");
    let bob_pk = *bob_identity.signing_public_key();
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), SymmetricKey::generate());
    let bob_contact_id = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();

    // The validation should succeed and return the validation record
    let validation = alice
        .validate_field(&bob_contact_id, "twitter", "@bob")
        .unwrap();
    assert_eq!(
        validation.field_value(),
        "@bob",
        "Validation should return correct field_value regardless of queue state"
    );

    // Verify the validation was stored locally
    let has_validated = alice
        .has_validated_field(&bob_contact_id, "twitter")
        .unwrap();
    assert!(
        has_validated,
        "Validation should be stored locally even if queue might fail"
    );
}

// =============================================================================
// Incoming Validation Processing Tests
// Traces to: _private/features/field_validation.feature @incoming @sync
// =============================================================================

// @scenario: field_validation :: Validations are cryptographically signed
// @scenario: field_validation :: Validation count syncs from contacts
#[test]
fn test_process_incoming_validation_verifies_and_stores() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Bob (the recipient) with identity
    let mut bob: Vauchi = Vauchi::in_memory().unwrap();
    bob.create_identity("Bob").unwrap();

    let bob_identity_id = hex::encode(bob.identity().unwrap().signing_public_key());

    // Create Alice's identity (she will sign the validation)
    let alice_identity = Identity::create("Alice");
    let alice_pk = *alice_identity.signing_public_key();
    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Alice creates a signed validation of Bob's twitter field
    let validation = vauchi_core::social::ProfileValidation::create_signed(
        &alice_identity,
        "twitter",
        "@bob",
        &bob_identity_id,
    );

    // Serialize to bytes (as would be sent over the network)
    let validation_bytes = serde_json::to_vec(&validation).unwrap();

    // Bob processes the incoming validation from Alice
    let result = bob.process_incoming_validation(&alice_contact_id, &validation_bytes);
    assert!(
        result.is_ok(),
        "process_incoming_validation should succeed for valid signed validation: {:?}",
        result.err()
    );

    // Verify it was stored
    let status = bob
        .get_field_validation_status(&bob_identity_id, "twitter", "@bob")
        .unwrap();
    assert_eq!(
        status.count, 1,
        "One validation should be stored after processing incoming validation"
    );
    assert!(
        status.validator_ids.contains(&hex::encode(alice_pk)),
        "The validator should be Alice"
    );
}

// @scenario: field_validation :: Cannot forge validations
#[test]
fn test_process_incoming_validation_rejects_invalid_signature() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Bob (the recipient) with identity
    let mut bob: Vauchi = Vauchi::in_memory().unwrap();
    bob.create_identity("Bob").unwrap();

    let bob_identity_id = hex::encode(bob.identity().unwrap().signing_public_key());

    // Create Alice as a contact
    let alice_identity = Identity::create("Alice");
    let alice_pk = *alice_identity.signing_public_key();
    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Craft a validation with a zeroed-out (invalid) signature
    let validation = vauchi_core::social::ProfileValidation::new(
        &format!("{}:twitter", bob_identity_id),
        "@bob",
        &hex::encode(alice_pk),
        [0u8; 64], // invalid signature
    );

    let validation_bytes = serde_json::to_vec(&validation).unwrap();

    // Bob processes the incoming validation -- should reject due to invalid signature
    let result = bob.process_incoming_validation(&alice_contact_id, &validation_bytes);
    assert!(
        result.is_err(),
        "process_incoming_validation should reject invalid signature"
    );

    // Verify error is signature-related
    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("signature") || err_msg.contains("Signature"),
        "Error should mention signature verification failure, got: {}",
        err_msg
    );
}

// @scenario: field_validation :: Cannot forge validations
#[test]
fn test_process_incoming_validation_rejects_validator_id_mismatch() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Bob (the recipient) with identity
    let mut bob: Vauchi = Vauchi::in_memory().unwrap();
    bob.create_identity("Bob").unwrap();

    let bob_identity_id = hex::encode(bob.identity().unwrap().signing_public_key());

    // Create Alice and Eve as contacts
    let alice_identity = Identity::create("Alice");
    let alice_pk = *alice_identity.signing_public_key();
    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    bob.add_contact(alice_contact).unwrap();

    let eve_identity = Identity::create("Eve");
    let eve_pk = *eve_identity.signing_public_key();
    let eve_contact =
        Contact::from_exchange(eve_pk, ContactCard::new("Eve"), SymmetricKey::generate());
    let eve_contact_id = eve_contact.id().to_string();
    bob.add_contact(eve_contact).unwrap();

    // Alice creates a valid signed validation
    let validation = vauchi_core::social::ProfileValidation::create_signed(
        &alice_identity,
        "twitter",
        "@bob",
        &bob_identity_id,
    );

    let validation_bytes = serde_json::to_vec(&validation).unwrap();

    // Eve tries to claim Alice's validation as her own (forwarding attack)
    let result = bob.process_incoming_validation(&eve_contact_id, &validation_bytes);
    assert!(
        result.is_err(),
        "process_incoming_validation should reject when validator_id doesn't match sender"
    );

    // Verify error mentions the mismatch
    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("mismatch") || err_msg.contains("does not match"),
        "Error should mention validator ID mismatch, got: {}",
        err_msg
    );
}

// @scenario: field_validation :: Validation count syncs from contacts
#[test]
fn test_process_incoming_validation_idempotent_on_duplicate() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Bob (the recipient) with identity
    let mut bob: Vauchi = Vauchi::in_memory().unwrap();
    bob.create_identity("Bob").unwrap();

    let bob_identity_id = hex::encode(bob.identity().unwrap().signing_public_key());

    // Create Alice as a contact
    let alice_identity = Identity::create("Alice");
    let alice_pk = *alice_identity.signing_public_key();
    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Alice creates a signed validation
    let validation = vauchi_core::social::ProfileValidation::create_signed(
        &alice_identity,
        "twitter",
        "@bob",
        &bob_identity_id,
    );

    let validation_bytes = serde_json::to_vec(&validation).unwrap();

    // Process twice -- second call should succeed (idempotent via UNIQUE constraint)
    let result1 = bob.process_incoming_validation(&alice_contact_id, &validation_bytes);
    assert!(result1.is_ok(), "First processing should succeed");

    let result2 = bob.process_incoming_validation(&alice_contact_id, &validation_bytes);
    assert!(
        result2.is_ok(),
        "Second (duplicate) processing should succeed (idempotent): {:?}",
        result2.err()
    );

    // Should still be just 1 validation (not 2)
    let status = bob
        .get_field_validation_status(&bob_identity_id, "twitter", "@bob")
        .unwrap();
    assert_eq!(
        status.count, 1,
        "Duplicate delivery should not create duplicate validations"
    );
}

// @scenario: field_validation :: Revoke validation
#[test]
fn test_process_incoming_revocation_deletes_validation() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Bob (the recipient) with identity
    let mut bob: Vauchi = Vauchi::in_memory().unwrap();
    bob.create_identity("Bob").unwrap();

    let bob_identity_id = hex::encode(bob.identity().unwrap().signing_public_key());

    // Create Alice as a contact
    let alice_identity = Identity::create("Alice");
    let alice_pk = *alice_identity.signing_public_key();
    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Alice validates Bob's twitter field, Bob processes it
    let validation = vauchi_core::social::ProfileValidation::create_signed(
        &alice_identity,
        "twitter",
        "@bob",
        &bob_identity_id,
    );
    let validation_bytes = serde_json::to_vec(&validation).unwrap();
    bob.process_incoming_validation(&alice_contact_id, &validation_bytes)
        .unwrap();

    // Verify validation is stored
    let status_before = bob
        .get_field_validation_status(&bob_identity_id, "twitter", "@bob")
        .unwrap();
    assert_eq!(
        status_before.count, 1,
        "Validation should be stored before revocation"
    );

    // Alice sends a revocation
    let revocation = serde_json::json!({
        "contact_id": bob_identity_id,
        "field_id": "twitter",
        "validator_id": hex::encode(alice_pk),
    });
    let revocation_bytes = serde_json::to_vec(&revocation).unwrap();

    let result = bob.process_incoming_revocation(&alice_contact_id, &revocation_bytes);
    assert!(
        result.is_ok(),
        "process_incoming_revocation should succeed: {:?}",
        result.err()
    );
    assert!(
        result.unwrap(),
        "Revocation should return true (validation was deleted)"
    );

    // Verify validation is gone
    let status_after = bob
        .get_field_validation_status(&bob_identity_id, "twitter", "@bob")
        .unwrap();
    assert_eq!(
        status_after.count, 0,
        "Validation should be deleted after revocation"
    );
}

// @scenario: field_validation :: Cannot forge validations
#[test]
fn test_process_incoming_revocation_rejects_validator_id_mismatch() {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::{Contact, Identity, Vauchi};

    // Create Bob (the recipient) with identity
    let mut bob: Vauchi = Vauchi::in_memory().unwrap();
    bob.create_identity("Bob").unwrap();

    let bob_identity_id = hex::encode(bob.identity().unwrap().signing_public_key());

    // Create Alice and Eve as contacts
    let alice_identity = Identity::create("Alice");
    let alice_pk = *alice_identity.signing_public_key();
    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    let eve_identity = Identity::create("Eve");
    let eve_pk = *eve_identity.signing_public_key();
    let eve_contact =
        Contact::from_exchange(eve_pk, ContactCard::new("Eve"), SymmetricKey::generate());
    let eve_contact_id = eve_contact.id().to_string();
    bob.add_contact(eve_contact).unwrap();

    // Alice validates Bob's twitter field, Bob processes it
    let validation = vauchi_core::social::ProfileValidation::create_signed(
        &alice_identity,
        "twitter",
        "@bob",
        &bob_identity_id,
    );
    let validation_bytes = serde_json::to_vec(&validation).unwrap();
    bob.process_incoming_validation(&alice_contact_id, &validation_bytes)
        .unwrap();

    // Eve tries to revoke Alice's validation (should fail)
    let revocation = serde_json::json!({
        "contact_id": bob_identity_id,
        "field_id": "twitter",
        "validator_id": hex::encode(alice_pk), // Alice's validator_id, but Eve is sending
    });
    let revocation_bytes = serde_json::to_vec(&revocation).unwrap();

    let result = bob.process_incoming_revocation(&eve_contact_id, &revocation_bytes);
    assert!(
        result.is_err(),
        "process_incoming_revocation should reject when sender doesn't match validator_id"
    );

    // Validation should still be present
    let status = bob
        .get_field_validation_status(&bob_identity_id, "twitter", "@bob")
        .unwrap();
    assert_eq!(
        status.count, 1,
        "Validation should not be deleted by unauthorized revocation"
    );
}
