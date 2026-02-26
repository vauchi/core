// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for device linking proximity verification.
//!
//! Verifies that `build_response()` refuses to release the master seed
//! unless proximity has been verified, and that the proximity challenge
//! derivation is deterministic and consistent across both sides.

use std::time::{SystemTime, UNIX_EPOCH};
use vauchi_core::exchange::{
    compute_confirmation_mac, DeviceLinkInitiator, DeviceLinkInitiatorRestored, DeviceLinkQR,
    DeviceLinkResponder, ExchangeError, ProximityProof,
};
use vauchi_core::identity::{DeviceRegistry, Identity};

fn create_test_registry(identity: &Identity) -> DeviceRegistry {
    let device_info = identity.device_info();
    let master_seed = [0x42u8; 32];
    DeviceRegistry::new(
        device_info.to_registered(&master_seed),
        identity.signing_keypair(),
    )
}

// @scenario: device_management.feature:Linking requires proximity verification
#[test]
fn test_build_response_rejected_without_proximity() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    // Initiator with a wrong proximity proof
    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();

    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // confirm_link() should fail because the proof has a wrong challenge
    let wrong_proof = ProximityProof::Ultrasonic {
        challenge_response: [0xFFu8; 16],
        verified_at: now_unix_secs(),
    };
    let result = initiator.confirm_link(&request, &wrong_proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Expected ProximityNotVerified, got: {:?}",
        result.err()
    );
}

// @scenario: device_management.feature:Linking requires proximity verification
#[test]
fn test_build_response_succeeds_after_proximity_verified() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();

    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // Valid proximity proof, then confirm should succeed
    let proof = ProximityProof::Ultrasonic {
        challenge_response: initiator.proximity_challenge(),
        verified_at: now_unix_secs(),
    };
    let (encrypted_response, updated_registry, new_device) =
        initiator.confirm_link(&request, &proof).unwrap();

    let response = responder.process_response(&encrypted_response).unwrap();

    assert_eq!(response.master_seed(), &master_seed);
    assert_eq!(response.display_name(), "Alice");
    assert_eq!(new_device.device_name(), "My Phone");
    assert_eq!(updated_registry.device_count(), 2);
}

#[test]
fn test_proximity_challenge_deterministic() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    // Same initiator should produce the same challenge every time
    let challenge1 = initiator.proximity_challenge();
    let challenge2 = initiator.proximity_challenge();
    assert_eq!(challenge1, challenge2);
    assert_eq!(challenge1.len(), 16);
}

#[test]
fn test_proximity_challenge_differs_per_session() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    // Two different initiators (different link_keys) should produce different challenges
    let initiator1 = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
    let initiator2 = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let challenge1 = initiator1.proximity_challenge();
    let challenge2 = initiator2.proximity_challenge();

    // Different random link_keys → different challenges (with overwhelming probability)
    assert_ne!(challenge1, challenge2);
}

// @scenario: device_management.feature:Verify device during linking
#[test]
fn test_both_sides_derive_same_challenge() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    // Responder scans the same QR
    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    // Both sides derive the same challenge from the shared link key
    let initiator_challenge = initiator.proximity_challenge();
    let responder_challenge = responder.proximity_challenge();

    assert_eq!(initiator_challenge, responder_challenge);
}

// @scenario: device_management.feature:Linking requires proximity verification
#[test]
fn test_restored_initiator_requires_proximity() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
    let qr_string = initiator.qr().to_data_string();

    // Restore initiator from saved QR
    let restored_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let restored_initiator =
        DeviceLinkInitiatorRestored::new(master_seed, &identity, registry, restored_qr);

    // Responder side
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();

    let (_confirmation, request) = restored_initiator
        .prepare_confirmation(&encrypted_request)
        .unwrap();

    // Should fail with wrong proximity proof
    let wrong_proof = ProximityProof::Ultrasonic {
        challenge_response: [0xFFu8; 16],
        verified_at: now_unix_secs(),
    };
    let result = restored_initiator.confirm_link(&request, &wrong_proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Expected ProximityNotVerified on restored initiator, got: {:?}",
        result.err()
    );
}

#[test]
#[allow(deprecated)]
fn test_deprecated_process_request_requires_proximity() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    // Initiator with wrong proximity proof
    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();

    // Deprecated process_request() should also enforce proximity
    let wrong_proof = ProximityProof::Ultrasonic {
        challenge_response: [0xFFu8; 16],
        verified_at: now_unix_secs(),
    };
    let result = initiator.process_request(&encrypted_request, &wrong_proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Expected ProximityNotVerified on deprecated API, got: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Evidence-based proximity proof tests (TDD red phase)
//
// These tests call confirm_link(&request, &proof) with TWO arguments.
// The current API only takes ONE argument (&request), so these tests
// will NOT compile until the API is updated in the next task.
// ---------------------------------------------------------------------------

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// @scenario: device_management.feature:Linking with ultrasonic proximity proof succeeds
#[test]
fn test_confirm_link_with_ultrasonic_proof_succeeds() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let challenge = initiator.proximity_challenge();

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    let proof = ProximityProof::Ultrasonic {
        challenge_response: challenge,
        verified_at: now_unix_secs(),
    };

    let (encrypted_response, updated_registry, new_device) =
        initiator.confirm_link(&request, &proof).unwrap();

    let response = responder.process_response(&encrypted_response).unwrap();

    assert_eq!(response.master_seed(), &master_seed);
    assert_eq!(response.display_name(), "Alice");
    assert_eq!(new_device.device_name(), "My Phone");
    assert_eq!(updated_registry.device_count(), 2);
}

// @scenario: device_management.feature:Linking with manual confirmation proof succeeds
#[test]
fn test_confirm_link_with_manual_proof_succeeds() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();
    let (confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    let proof = ProximityProof::ManualConfirmation {
        confirmation_code_mac: compute_confirmation_mac(
            initiator.qr().link_key(),
            &confirmation.confirmation_code,
        ),
        confirmed_at: now_unix_secs(),
    };

    let (encrypted_response, updated_registry, new_device) =
        initiator.confirm_link(&request, &proof).unwrap();

    let response = responder.process_response(&encrypted_response).unwrap();

    assert_eq!(response.master_seed(), &master_seed);
    assert_eq!(response.display_name(), "Alice");
    assert_eq!(new_device.device_name(), "My Phone");
    assert_eq!(updated_registry.device_count(), 2);
}

// @scenario: device_management.feature:Expired ultrasonic proof is rejected
#[test]
fn test_confirm_link_with_expired_ultrasonic_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let challenge = initiator.proximity_challenge();

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // Timestamp 120 seconds in the past — should be expired
    let proof = ProximityProof::Ultrasonic {
        challenge_response: challenge,
        verified_at: now_unix_secs().saturating_sub(120),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityExpired)),
        "Expected ProximityExpired for stale ultrasonic proof, got: {:?}",
        result.err()
    );
}

// @scenario: device_management.feature:Expired manual confirmation is rejected
#[test]
fn test_confirm_link_with_expired_manual_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();
    let (confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // Correct MAC but timestamp 120 seconds in the past — should be expired
    let proof = ProximityProof::ManualConfirmation {
        confirmation_code_mac: compute_confirmation_mac(
            initiator.qr().link_key(),
            &confirmation.confirmation_code,
        ),
        confirmed_at: now_unix_secs().saturating_sub(120),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityExpired)),
        "Expected ProximityExpired for stale manual proof, got: {:?}",
        result.err()
    );
}

// @scenario: device_management.feature:Wrong ultrasonic challenge is rejected
#[test]
fn test_confirm_link_with_wrong_challenge_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // Wrong challenge response — attacker doesn't know the correct challenge
    let proof = ProximityProof::Ultrasonic {
        challenge_response: [0xFFu8; 16],
        verified_at: now_unix_secs(),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Expected ProximityNotVerified for wrong challenge, got: {:?}",
        result.err()
    );
}

// @scenario: device_management.feature:Wrong manual confirmation MAC is rejected
#[test]
fn test_confirm_link_with_wrong_mac_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // Wrong MAC — attacker doesn't know the confirmation code
    let proof = ProximityProof::ManualConfirmation {
        confirmation_code_mac: [0xABu8; 32],
        confirmed_at: now_unix_secs(),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Expected ProximityNotVerified for wrong MAC, got: {:?}",
        result.err()
    );
}
