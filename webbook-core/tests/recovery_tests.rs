//! Recovery Module Tests
//!
//! Tests for contact recovery via social vouching.
//! Following TDD: write tests first, then implement.

use webbook_core::recovery::{
    RecoveryClaim, RecoveryError, RecoveryProof, RecoverySettings, RecoveryVoucher,
    VerificationResult,
};
use webbook_core::{Contact, ContactCard, Identity, SigningKeyPair, SymmetricKey};

// =============================================================================
// RecoveryClaim Tests
// =============================================================================

#[test]
fn test_recovery_claim_creation() {
    // Alice lost her device and creates a new identity
    let old_identity = Identity::create("Alice (old)");
    let new_identity = Identity::create("Alice (new)");

    let claim = RecoveryClaim::new(
        old_identity.signing_public_key(),
        new_identity.signing_public_key(),
    );

    assert_eq!(claim.old_pk(), old_identity.signing_public_key());
    assert_eq!(claim.new_pk(), new_identity.signing_public_key());
    assert!(claim.timestamp() > 0);
}

#[test]
fn test_recovery_claim_serialization() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    let claim = RecoveryClaim::new(&old_pk, &new_pk);

    // Serialize to bytes (for QR code)
    let bytes = claim.to_bytes();
    assert!(!bytes.is_empty());

    // Deserialize from bytes
    let restored = RecoveryClaim::from_bytes(&bytes).unwrap();
    assert_eq!(restored.old_pk(), &old_pk);
    assert_eq!(restored.new_pk(), &new_pk);
}

#[test]
fn test_recovery_claim_invalid_bytes() {
    // Too short
    let result = RecoveryClaim::from_bytes(&[0u8; 10]);
    assert!(result.is_err());

    // Empty
    let result = RecoveryClaim::from_bytes(&[]);
    assert!(result.is_err());
}

// =============================================================================
// RecoveryVoucher Tests
// =============================================================================

#[test]
fn test_recovery_voucher_creation() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);

    assert_eq!(voucher.old_pk(), &old_pk);
    assert_eq!(voucher.new_pk(), &new_pk);
    assert_eq!(voucher.voucher_pk(), voucher_keypair.public_key().as_bytes());
}

#[test]
fn test_recovery_voucher_signature_valid() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);

    // Signature should verify
    assert!(voucher.verify());
}

#[test]
fn test_recovery_voucher_signature_invalid_on_tamper() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let mut voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);

    // Tamper with new_pk
    let tampered_pk = [0x03u8; 32];
    voucher.set_new_pk_for_testing(&tampered_pk);

    // Signature should fail verification
    assert!(!voucher.verify());
}

#[test]
fn test_recovery_voucher_serialization() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);

    let bytes = voucher.to_bytes();
    let restored = RecoveryVoucher::from_bytes(&bytes).unwrap();

    assert_eq!(restored.old_pk(), voucher.old_pk());
    assert_eq!(restored.new_pk(), voucher.new_pk());
    assert_eq!(restored.voucher_pk(), voucher.voucher_pk());
    assert!(restored.verify());
}

// =============================================================================
// RecoveryProof Tests
// =============================================================================

#[test]
fn test_recovery_proof_creation() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    let proof = RecoveryProof::new(&old_pk, &new_pk, 3);

    assert_eq!(proof.old_pk(), &old_pk);
    assert_eq!(proof.new_pk(), &new_pk);
    assert_eq!(proof.threshold(), 3);
    assert_eq!(proof.voucher_count(), 0);
}

#[test]
fn test_recovery_proof_add_voucher() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);
    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);

    proof.add_voucher(voucher).unwrap();

    assert_eq!(proof.voucher_count(), 1);
}

#[test]
fn test_recovery_proof_reject_mismatched_keys() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let wrong_old_pk = [0x03u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);
    let voucher = RecoveryVoucher::create(&wrong_old_pk, &new_pk, &voucher_keypair);

    let result = proof.add_voucher(voucher);
    assert!(matches!(result, Err(RecoveryError::MismatchedKeys)));
}

#[test]
fn test_recovery_proof_reject_duplicate_voucher() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);
    let voucher1 = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);
    let voucher2 = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);

    proof.add_voucher(voucher1).unwrap();
    let result = proof.add_voucher(voucher2);

    assert!(matches!(result, Err(RecoveryError::DuplicateVoucher)));
}

#[test]
fn test_recovery_proof_threshold_not_met() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);

    // Add only 2 vouchers
    for _ in 0..2 {
        let voucher_keypair = SigningKeyPair::generate();
        let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);
        proof.add_voucher(voucher).unwrap();
    }

    let result = proof.validate();
    assert!(matches!(result, Err(RecoveryError::InsufficientVouchers(_))));
}

#[test]
fn test_recovery_proof_threshold_met() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);

    // Add 3 vouchers
    for _ in 0..3 {
        let voucher_keypair = SigningKeyPair::generate();
        let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);
        proof.add_voucher(voucher).unwrap();
    }

    let result = proof.validate();
    assert!(result.is_ok());
}

#[test]
fn test_recovery_proof_serialization() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 2);

    // Add vouchers
    for _ in 0..2 {
        let voucher_keypair = SigningKeyPair::generate();
        let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair);
        proof.add_voucher(voucher).unwrap();
    }

    let bytes = proof.to_bytes();
    let restored = RecoveryProof::from_bytes(&bytes).unwrap();

    assert_eq!(restored.old_pk(), proof.old_pk());
    assert_eq!(restored.new_pk(), proof.new_pk());
    assert_eq!(restored.threshold(), proof.threshold());
    assert_eq!(restored.voucher_count(), proof.voucher_count());
    assert!(restored.validate().is_ok());
}

// =============================================================================
// Verification Result Tests
// =============================================================================

fn create_test_contact(keypair: &SigningKeyPair) -> Contact {
    let card = ContactCard::new("Test Contact");
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(*keypair.public_key().as_bytes(), card, shared_key)
}

#[test]
fn test_verification_high_confidence() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    // Create contacts who will vouch
    let bob_keypair = SigningKeyPair::generate();
    let charlie_keypair = SigningKeyPair::generate();
    let dave_keypair = SigningKeyPair::generate();

    let bob = create_test_contact(&bob_keypair);
    let charlie = create_test_contact(&charlie_keypair);

    // Create proof with 3 vouchers (2 are mutual contacts)
    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &bob_keypair))
        .unwrap();
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &charlie_keypair))
        .unwrap();
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &dave_keypair))
        .unwrap();

    let contacts = vec![bob, charlie];
    let settings = RecoverySettings::default();

    let result = proof.verify_for_contact(&contacts, &settings);

    assert!(matches!(result, VerificationResult::HighConfidence { .. }));
    if let VerificationResult::HighConfidence {
        mutual_vouchers,
        total_vouchers,
    } = result
    {
        assert_eq!(mutual_vouchers.len(), 2);
        assert_eq!(total_vouchers, 3);
    }
}

#[test]
fn test_verification_medium_confidence() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    // Create 1 mutual contact
    let bob_keypair = SigningKeyPair::generate();
    let charlie_keypair = SigningKeyPair::generate();
    let dave_keypair = SigningKeyPair::generate();

    let bob = create_test_contact(&bob_keypair);

    // Create proof with 3 vouchers (only 1 is mutual contact)
    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &bob_keypair))
        .unwrap();
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &charlie_keypair))
        .unwrap();
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &dave_keypair))
        .unwrap();

    let contacts = vec![bob];
    let settings = RecoverySettings::default();

    let result = proof.verify_for_contact(&contacts, &settings);

    assert!(matches!(result, VerificationResult::MediumConfidence { .. }));
}

#[test]
fn test_verification_low_confidence() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    // No mutual contacts
    let bob_keypair = SigningKeyPair::generate();
    let charlie_keypair = SigningKeyPair::generate();
    let dave_keypair = SigningKeyPair::generate();

    // Create proof with 3 vouchers (none are mutual contacts)
    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &bob_keypair))
        .unwrap();
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &charlie_keypair))
        .unwrap();
    proof
        .add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, &dave_keypair))
        .unwrap();

    let contacts: Vec<Contact> = vec![]; // No contacts
    let settings = RecoverySettings::default();

    let result = proof.verify_for_contact(&contacts, &settings);

    assert!(matches!(result, VerificationResult::LowConfidence { .. }));
}

// =============================================================================
// RecoverySettings Tests
// =============================================================================

#[test]
fn test_recovery_settings_default() {
    let settings = RecoverySettings::default();

    assert_eq!(settings.recovery_threshold(), 3);
    assert_eq!(settings.verification_threshold(), 2);
}

#[test]
fn test_recovery_settings_custom() {
    let settings = RecoverySettings::new(5, 3).unwrap();

    assert_eq!(settings.recovery_threshold(), 5);
    assert_eq!(settings.verification_threshold(), 3);
}

// =============================================================================
// Recovery Threshold Limits Tests (Scenario: Recovery threshold limits)
// =============================================================================

#[test]
fn test_recovery_threshold_minimum() {
    // Scenario: Recovery threshold limits
    // When I try to set my recovery threshold to 0
    // Then the operation should fail with "Recovery threshold must be at least 1"
    let result = RecoverySettings::new(0, 2);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("at least 1"));
}

#[test]
fn test_recovery_threshold_maximum() {
    // Scenario: Recovery threshold limits
    // When I try to set my recovery threshold to 20
    // Then the operation should fail with "Recovery threshold cannot exceed 10"
    let result = RecoverySettings::new(20, 2);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("cannot exceed 10"));
}

#[test]
fn test_recovery_threshold_valid_range() {
    // Valid thresholds: 1-10
    for threshold in 1..=10 {
        // verification_threshold must be <= recovery_threshold
        let verification = threshold.min(2);
        let result = RecoverySettings::new(threshold, verification);
        assert!(result.is_ok(), "threshold {} should be valid", threshold);
    }
}

#[test]
fn test_verification_threshold_minimum() {
    // Verification threshold must be at least 1
    let result = RecoverySettings::new(3, 0);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("at least 1"));
}

#[test]
fn test_verification_threshold_maximum() {
    // Verification threshold cannot exceed recovery threshold
    let result = RecoverySettings::new(3, 5);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("cannot exceed"));
}

// =============================================================================
// Voucher Timestamp Validation Tests (Scenario: Voucher timestamp validation)
// =============================================================================

#[test]
fn test_voucher_claim_not_expired() {
    // Scenario: Voucher timestamp validation
    // Fresh claim should be accepted
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let claim = RecoveryClaim::new(&old_pk, &new_pk);

    // Create voucher from fresh claim
    let result = RecoveryVoucher::create_from_claim(&claim, &voucher_keypair);
    assert!(result.is_ok());
}

#[test]
fn test_voucher_claim_expired_48_hours() {
    // Scenario: Voucher timestamp validation
    // When Bob vouches 48 hours later
    // Then the voucher is rejected as expired
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    // Create a claim with timestamp 49 hours ago
    let claim = RecoveryClaim::new_with_timestamp(&old_pk, &new_pk, expired_timestamp(49 * 3600));

    // Should reject voucher creation for expired claim
    let result = RecoveryVoucher::create_from_claim(&claim, &voucher_keypair);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("expired") || err.to_string().contains("Expired"));
}

#[test]
fn test_voucher_claim_just_under_48_hours() {
    // Claim that's 47 hours old should still be valid
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    // Create a claim with timestamp 47 hours ago
    let claim = RecoveryClaim::new_with_timestamp(&old_pk, &new_pk, expired_timestamp(47 * 3600));

    // Should accept voucher creation
    let result = RecoveryVoucher::create_from_claim(&claim, &voucher_keypair);
    assert!(result.is_ok());
}

/// Helper to create a timestamp N seconds in the past.
fn expired_timestamp(seconds_ago: u64) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
    now.saturating_sub(seconds_ago)
}

// =============================================================================
// Self-Vouching Prevention Tests (Scenario: Cannot vouch for own recovery)
// =============================================================================

#[test]
fn test_cannot_vouch_for_self() {
    // Scenario: Cannot vouch for own recovery
    // Given Alice claims recovery from "pk_old" to "pk_new"
    // When Alice tries to vouch for herself (using pk_new)
    // Then the self-voucher is rejected
    let alice_old = Identity::create("Alice (old)");
    let alice_new = Identity::create("Alice (new)");

    let claim = RecoveryClaim::new(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
    );

    // Alice tries to vouch for herself using her new identity
    let result = RecoveryVoucher::create_from_claim(&claim, alice_new.signing_keypair());

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("self") || err.to_string().contains("Self"));
}

#[test]
fn test_proof_rejects_self_voucher() {
    // Even if a self-voucher is somehow created, the proof should reject it
    let old_pk = [0x01u8; 32];
    let new_keypair = SigningKeyPair::generate();
    let new_pk = *new_keypair.public_key().as_bytes();

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 3);

    // Create a voucher where voucher_pk == new_pk (self-vouching)
    let self_voucher = RecoveryVoucher::create(&old_pk, &new_pk, &new_keypair);

    let result = proof.add_voucher(self_voucher);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("self") || err.to_string().contains("Self"));
}

#[test]
fn test_other_contacts_can_still_vouch() {
    // Verify that other contacts can still vouch (not affected by self-vouch check)
    let alice_old = Identity::create("Alice (old)");
    let alice_new = Identity::create("Alice (new)");
    let bob = Identity::create("Bob");

    let claim = RecoveryClaim::new(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
    );

    // Bob vouches for Alice - should succeed
    let result = RecoveryVoucher::create_from_claim(&claim, bob.signing_keypair());
    assert!(result.is_ok());
}

// =============================================================================
// End-to-End Flow Tests
// =============================================================================

#[test]
fn test_full_recovery_flow() {
    // 1. Alice had an old identity
    let alice_old = Identity::create("Alice");
    let alice_old_pk = *alice_old.signing_public_key();

    // 2. Alice loses device, creates new identity
    let alice_new = Identity::create("Alice (recovered)");
    let alice_new_pk = *alice_new.signing_public_key();

    // 3. Alice creates recovery claim
    let claim = RecoveryClaim::new(&alice_old_pk, &alice_new_pk);

    // 4. Bob, Charlie, Dave vouch for Alice in person
    let bob = Identity::create("Bob");
    let charlie = Identity::create("Charlie");
    let dave = Identity::create("Dave");

    let voucher_bob =
        RecoveryVoucher::create(claim.old_pk(), claim.new_pk(), bob.signing_keypair());
    let voucher_charlie =
        RecoveryVoucher::create(claim.old_pk(), claim.new_pk(), charlie.signing_keypair());
    let voucher_dave =
        RecoveryVoucher::create(claim.old_pk(), claim.new_pk(), dave.signing_keypair());

    // 5. Alice aggregates vouchers into proof
    let mut proof = RecoveryProof::new(claim.old_pk(), claim.new_pk(), 3);
    proof.add_voucher(voucher_bob).unwrap();
    proof.add_voucher(voucher_charlie).unwrap();
    proof.add_voucher(voucher_dave).unwrap();

    // 6. Proof is valid
    assert!(proof.validate().is_ok());

    // 7. Proof can be serialized and restored
    let proof_bytes = proof.to_bytes();
    let restored_proof = RecoveryProof::from_bytes(&proof_bytes).unwrap();
    assert!(restored_proof.validate().is_ok());
}
