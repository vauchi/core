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

// =============================================================================
// Recovery Acceptance Flow Tests (Scenario: Accept recovery and reconnect)
// =============================================================================

#[test]
fn test_accept_recovery_updates_contact() {
    // Scenario: Accept recovery and reconnect
    // Given John has Alice as a contact with old_pk
    // When John accepts Alice's recovery proof
    // Then John's contact record for Alice is updated with new_pk
    use webbook_core::contact::Contact;
    use webbook_core::contact_card::ContactCard;
    use webbook_core::crypto::SymmetricKey;

    let alice_old_pk = [0x01u8; 32];
    let alice_new_pk = [0x02u8; 32];
    let old_shared_key = SymmetricKey::generate();
    let new_shared_key = SymmetricKey::generate();

    let card = ContactCard::new("Alice");
    let mut contact = Contact::from_exchange(alice_old_pk, card, old_shared_key.clone());

    let old_id = contact.id().to_string();
    assert_eq!(contact.public_key(), &alice_old_pk);

    // Accept recovery
    contact.accept_recovery(alice_new_pk, new_shared_key.clone());

    // Verify contact is updated
    assert_eq!(contact.public_key(), &alice_new_pk);
    assert_ne!(contact.id(), old_id); // ID changes because it's based on public key
    assert_eq!(contact.display_name(), "Alice"); // Name stays the same
}

#[test]
fn test_accept_recovery_discards_old_shared_secret() {
    // Scenario: Accept recovery and reconnect
    // And the old shared secret is discarded
    use webbook_core::contact::Contact;
    use webbook_core::contact_card::ContactCard;
    use webbook_core::crypto::SymmetricKey;

    let alice_old_pk = [0x01u8; 32];
    let alice_new_pk = [0x02u8; 32];
    let old_shared_key = SymmetricKey::generate();
    let new_shared_key = SymmetricKey::generate();

    let card = ContactCard::new("Alice");
    let mut contact = Contact::from_exchange(alice_old_pk, card, old_shared_key.clone());

    // Get reference to old key for comparison
    let old_key_bytes = contact.shared_key().as_bytes().to_vec();

    // Accept recovery
    contact.accept_recovery(alice_new_pk, new_shared_key.clone());

    // New shared key should be different
    let new_key_bytes = contact.shared_key().as_bytes();
    assert_ne!(old_key_bytes.as_slice(), new_key_bytes);
}

#[test]
fn test_accept_recovery_resets_fingerprint_verification() {
    // After recovery, fingerprint needs to be re-verified
    use webbook_core::contact::Contact;
    use webbook_core::contact_card::ContactCard;
    use webbook_core::crypto::SymmetricKey;

    let alice_old_pk = [0x01u8; 32];
    let alice_new_pk = [0x02u8; 32];
    let old_shared_key = SymmetricKey::generate();
    let new_shared_key = SymmetricKey::generate();

    let card = ContactCard::new("Alice");
    let mut contact = Contact::from_exchange(alice_old_pk, card, old_shared_key);

    // Mark as verified
    contact.mark_fingerprint_verified();
    assert!(contact.is_fingerprint_verified());

    // Accept recovery
    contact.accept_recovery(alice_new_pk, new_shared_key);

    // Fingerprint verification should be reset
    assert!(!contact.is_fingerprint_verified());
}

#[test]
fn test_accept_recovery_with_new_card() {
    // Scenario: Contact card is refreshed after recovery
    use webbook_core::contact::Contact;
    use webbook_core::contact_card::ContactCard;
    use webbook_core::crypto::SymmetricKey;

    let alice_old_pk = [0x01u8; 32];
    let alice_new_pk = [0x02u8; 32];
    let old_shared_key = SymmetricKey::generate();
    let new_shared_key = SymmetricKey::generate();

    let old_card = ContactCard::new("Alice");
    let mut contact = Contact::from_exchange(alice_old_pk, old_card, old_shared_key);

    let new_card = ContactCard::new("Alice Smith");

    // Accept recovery with updated card
    contact.accept_recovery_with_card(alice_new_pk, new_shared_key, new_card);

    assert_eq!(contact.display_name(), "Alice Smith");
    assert_eq!(contact.card().display_name(), "Alice Smith");
}

// =============================================================================
// Remind Me Later Tests (Scenario: Remind me later)
// =============================================================================

#[test]
fn test_recovery_reminder_creation() {
    // Scenario: Remind me later
    // When John chooses "Remind Me Later"
    // Then the notification is dismissed
    use webbook_core::recovery::RecoveryReminder;

    let old_pk = [0x01u8; 32];
    let reminder = RecoveryReminder::new(old_pk);

    assert_eq!(reminder.old_pk(), &old_pk);
    assert!(!reminder.is_due());
}

#[test]
fn test_recovery_reminder_default_7_days() {
    // And John is reminded after 7 days
    use webbook_core::recovery::RecoveryReminder;

    let old_pk = [0x01u8; 32];
    let reminder = RecoveryReminder::new(old_pk);

    // Default reminder period is 7 days
    assert_eq!(reminder.reminder_days(), 7);
}

#[test]
fn test_recovery_reminder_custom_period() {
    // And John can adjust the reminder period
    use webbook_core::recovery::RecoveryReminder;

    let old_pk = [0x01u8; 32];
    let reminder = RecoveryReminder::with_days(old_pk, 14);

    assert_eq!(reminder.reminder_days(), 14);
}

#[test]
fn test_recovery_reminder_is_due_after_period() {
    use webbook_core::recovery::RecoveryReminder;

    let old_pk = [0x01u8; 32];
    // Create a reminder that was set 8 days ago (past the 7-day default)
    let reminder = RecoveryReminder::new_with_timestamp(old_pk, days_ago(8), 7);

    assert!(reminder.is_due());
}

#[test]
fn test_recovery_reminder_not_due_before_period() {
    use webbook_core::recovery::RecoveryReminder;

    let old_pk = [0x01u8; 32];
    // Create a reminder that was set 5 days ago (before the 7-day default)
    let reminder = RecoveryReminder::new_with_timestamp(old_pk, days_ago(5), 7);

    assert!(!reminder.is_due());
}

#[test]
fn test_recovery_reminder_snooze() {
    use webbook_core::recovery::RecoveryReminder;

    let old_pk = [0x01u8; 32];
    // Create a reminder that is due
    let mut reminder = RecoveryReminder::new_with_timestamp(old_pk, days_ago(8), 7);
    assert!(reminder.is_due());

    // Snooze for another 7 days
    reminder.snooze(7);

    // Should no longer be due
    assert!(!reminder.is_due());
}

/// Helper to create a timestamp N days in the past.
fn days_ago(days: u64) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
    now.saturating_sub(days * 24 * 60 * 60)
}

// =============================================================================
// Conflicting Recovery Claims Tests (Scenario: Detect conflicting recovery claims)
// =============================================================================

#[test]
fn test_detect_conflicting_claims() {
    // Scenario: Detect conflicting recovery claims
    // Given Alice uploads a recovery proof for "pk_old" -> "pk_new_1"
    // When an attacker uploads a recovery proof for "pk_old" -> "pk_new_2"
    // Then contacts see a conflict warning
    use webbook_core::recovery::RecoveryConflict;

    let old_pk = [0x01u8; 32];
    let new_pk_1 = [0x02u8; 32];
    let new_pk_2 = [0x03u8; 32];

    // Create two proofs for the same old_pk with different new_pks
    let bob = Identity::create("Bob");
    let charlie = Identity::create("Charlie");
    let dave = Identity::create("Dave");

    let mut proof1 = RecoveryProof::new(&old_pk, &new_pk_1, 2);
    proof1.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk_1, bob.signing_keypair())).unwrap();
    proof1.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk_1, charlie.signing_keypair())).unwrap();

    let mut proof2 = RecoveryProof::new(&old_pk, &new_pk_2, 2);
    proof2.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk_2, dave.signing_keypair())).unwrap();
    proof2.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk_2, bob.signing_keypair())).unwrap();

    // Detect conflict
    let conflict = RecoveryConflict::detect(&[proof1.clone(), proof2.clone()]);

    assert!(conflict.is_some());
    let conflict = conflict.unwrap();
    assert_eq!(conflict.old_pk(), &old_pk);
    assert_eq!(conflict.claims().len(), 2);
}

#[test]
fn test_no_conflict_single_proof() {
    use webbook_core::recovery::RecoveryConflict;

    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    let bob = Identity::create("Bob");
    let charlie = Identity::create("Charlie");

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 2);
    proof.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, bob.signing_keypair())).unwrap();
    proof.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, charlie.signing_keypair())).unwrap();

    // No conflict with single proof
    let conflict = RecoveryConflict::detect(&[proof]);
    assert!(conflict.is_none());
}

#[test]
fn test_no_conflict_same_new_pk() {
    // Multiple proofs with same old_pk AND same new_pk is not a conflict
    use webbook_core::recovery::RecoveryConflict;

    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    let bob = Identity::create("Bob");
    let charlie = Identity::create("Charlie");

    let mut proof1 = RecoveryProof::new(&old_pk, &new_pk, 1);
    proof1.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, bob.signing_keypair())).unwrap();

    let mut proof2 = RecoveryProof::new(&old_pk, &new_pk, 1);
    proof2.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk, charlie.signing_keypair())).unwrap();

    // No conflict since both have same new_pk
    let conflict = RecoveryConflict::detect(&[proof1, proof2]);
    assert!(conflict.is_none());
}

#[test]
fn test_conflict_claim_info() {
    use webbook_core::recovery::RecoveryConflict;

    let old_pk = [0x01u8; 32];
    let new_pk_1 = [0x02u8; 32];
    let new_pk_2 = [0x03u8; 32];

    let bob = Identity::create("Bob");
    let charlie = Identity::create("Charlie");

    let mut proof1 = RecoveryProof::new(&old_pk, &new_pk_1, 2);
    proof1.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk_1, bob.signing_keypair())).unwrap();
    proof1.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk_1, charlie.signing_keypair())).unwrap();

    let mut proof2 = RecoveryProof::new(&old_pk, &new_pk_2, 1);
    proof2.add_voucher(RecoveryVoucher::create(&old_pk, &new_pk_2, bob.signing_keypair())).unwrap();

    let conflict = RecoveryConflict::detect(&[proof1, proof2]).unwrap();

    // Claims should have correct voucher counts
    let claims = conflict.claims();
    assert!(claims.iter().any(|c| c.new_pk() == &new_pk_1 && c.voucher_count() == 2));
    assert!(claims.iter().any(|c| c.new_pk() == &new_pk_2 && c.voucher_count() == 1));
}

// =============================================================================
// Recovery Revocation Tests (Scenario: Revoke recovery proof)
// =============================================================================

#[test]
fn test_create_revocation() {
    // Scenario: Revoke recovery proof
    // When Alice signs a revocation with her old private key
    use webbook_core::recovery::RecoveryRevocation;

    let alice_old = Identity::create("Alice (old)");
    let alice_new = Identity::create("Alice (new)");

    let revocation = RecoveryRevocation::create(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
        alice_old.signing_keypair(),
    );

    assert_eq!(revocation.old_pk(), alice_old.signing_public_key());
    assert_eq!(revocation.new_pk(), alice_new.signing_public_key());
}

#[test]
fn test_revocation_signature_valid() {
    use webbook_core::recovery::RecoveryRevocation;

    let alice_old = Identity::create("Alice (old)");
    let alice_new = Identity::create("Alice (new)");

    let revocation = RecoveryRevocation::create(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
        alice_old.signing_keypair(),
    );

    // Signature should verify
    assert!(revocation.verify());
}

#[test]
fn test_revocation_signature_invalid_wrong_key() {
    use webbook_core::recovery::RecoveryRevocation;

    let alice_old = Identity::create("Alice (old)");
    let alice_new = Identity::create("Alice (new)");
    let attacker = Identity::create("Attacker");

    // Attacker tries to create revocation (but doesn't have old private key)
    let revocation = RecoveryRevocation::create(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
        attacker.signing_keypair(), // Wrong key!
    );

    // Signature should NOT verify
    assert!(!revocation.verify());
}

#[test]
fn test_revocation_applies_to_proof() {
    use webbook_core::recovery::RecoveryRevocation;

    let alice_old = Identity::create("Alice (old)");
    let alice_new = Identity::create("Alice (new)");
    let bob = Identity::create("Bob");

    // Create a recovery proof
    let mut proof = RecoveryProof::new(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
        1,
    );
    proof.add_voucher(RecoveryVoucher::create(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
        bob.signing_keypair(),
    )).unwrap();

    // Create revocation
    let revocation = RecoveryRevocation::create(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
        alice_old.signing_keypair(),
    );

    // Revocation should apply to this proof
    assert!(revocation.applies_to(&proof));
}

#[test]
fn test_revocation_does_not_apply_different_proof() {
    use webbook_core::recovery::RecoveryRevocation;

    let alice_old = Identity::create("Alice (old)");
    let alice_new = Identity::create("Alice (new)");
    let alice_other = Identity::create("Alice (other)");
    let bob = Identity::create("Bob");

    // Create a recovery proof for different new_pk
    let mut proof = RecoveryProof::new(
        alice_old.signing_public_key(),
        alice_other.signing_public_key(), // Different new_pk
        1,
    );
    proof.add_voucher(RecoveryVoucher::create(
        alice_old.signing_public_key(),
        alice_other.signing_public_key(),
        bob.signing_keypair(),
    )).unwrap();

    // Create revocation for original new_pk
    let revocation = RecoveryRevocation::create(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
        alice_old.signing_keypair(),
    );

    // Revocation should NOT apply to this proof (different new_pk)
    assert!(!revocation.applies_to(&proof));
}

#[test]
fn test_revocation_serialization() {
    use webbook_core::recovery::RecoveryRevocation;

    let alice_old = Identity::create("Alice (old)");
    let alice_new = Identity::create("Alice (new)");

    let revocation = RecoveryRevocation::create(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
        alice_old.signing_keypair(),
    );

    let bytes = revocation.to_bytes();
    let restored = RecoveryRevocation::from_bytes(&bytes).unwrap();

    assert_eq!(restored.old_pk(), revocation.old_pk());
    assert_eq!(restored.new_pk(), revocation.new_pk());
    assert!(restored.verify());
}
