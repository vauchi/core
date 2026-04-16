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
    DeviceLinkInitiator, DeviceLinkInitiatorRestored, DeviceLinkQR, DeviceLinkResponder,
    ExchangeError, ProximityProof, compute_confirmation_mac,
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

// @scenario: device_management :: Linking requires proximity verification
// @scenario: device_management :: Prevent unauthorized device linking
// @internal
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

// @scenario: device_management :: Linking requires proximity verification
// @internal
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

// @internal
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

// @internal
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

// @scenario: device_management :: Verify device during linking
// @internal
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

// @scenario: device_management :: Linking requires proximity verification
// @internal
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

// @internal
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

// @scenario: device_management :: Linking with ultrasonic proximity proof succeeds
// @internal
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

// @scenario: device_management :: Linking with manual confirmation proof succeeds
// @internal
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

// @scenario: device_management :: Expired ultrasonic proof is rejected
// @internal
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

// @scenario: device_management :: Expired manual confirmation is rejected
// @internal
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

// @scenario: device_management :: Wrong ultrasonic challenge is rejected
// @internal
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

// @scenario: device_management :: Wrong manual confirmation MAC is rejected
// @internal
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

// ---------------------------------------------------------------------------
// DL-3: Cross-session replay rejection tests
// ---------------------------------------------------------------------------

// @scenario: device_management :: Cross-session replay attack prevented
// @internal
#[test]
fn test_cross_session_replay_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    // Session A — get the valid challenge
    let initiator_a = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
    let challenge_a = initiator_a.proximity_challenge();

    // Session B — different QR, different link_key
    let initiator_b = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string_b = initiator_b.qr().to_data_string();
    let scanned_qr_b = DeviceLinkQR::from_data_string(&qr_string_b).unwrap();
    let mut responder_b = DeviceLinkResponder::from_qr(scanned_qr_b, "Phone".to_string()).unwrap();
    let encrypted_request_b = responder_b.create_request().unwrap();
    let (_confirmation_b, request_b) = initiator_b
        .prepare_confirmation(&encrypted_request_b)
        .unwrap();

    // Replay Session A's proof in Session B
    let proof_from_a = ProximityProof::Ultrasonic {
        challenge_response: challenge_a,
        verified_at: now_unix_secs(),
    };

    let result = initiator_b.confirm_link(&request_b, &proof_from_a);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Replaying a proof from Session A in Session B must be rejected, got: {:?}",
        result.err()
    );
}

// @scenario: device_management :: Cross-session replay attack prevented
// @internal
#[test]
fn test_cross_session_manual_mac_replay_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    // Session A — compute a valid manual MAC
    let initiator_a = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
    let qr_string_a = initiator_a.qr().to_data_string();
    let scanned_qr_a = DeviceLinkQR::from_data_string(&qr_string_a).unwrap();
    let mut responder_a = DeviceLinkResponder::from_qr(scanned_qr_a, "Phone".to_string()).unwrap();
    let encrypted_request_a = responder_a.create_request().unwrap();
    let (confirmation_a, _request_a) = initiator_a
        .prepare_confirmation(&encrypted_request_a)
        .unwrap();
    let mac_from_a = compute_confirmation_mac(
        initiator_a.qr().link_key(),
        &confirmation_a.confirmation_code,
    );

    // Session B — different link_key and different confirmation code
    let initiator_b = DeviceLinkInitiator::new(master_seed, &identity, registry);
    let qr_string_b = initiator_b.qr().to_data_string();
    let scanned_qr_b = DeviceLinkQR::from_data_string(&qr_string_b).unwrap();
    let mut responder_b = DeviceLinkResponder::from_qr(scanned_qr_b, "Phone".to_string()).unwrap();
    let encrypted_request_b = responder_b.create_request().unwrap();
    let (_confirmation_b, request_b) = initiator_b
        .prepare_confirmation(&encrypted_request_b)
        .unwrap();

    // Replay Session A's MAC in Session B
    let proof_from_a = ProximityProof::ManualConfirmation {
        confirmation_code_mac: mac_from_a,
        confirmed_at: now_unix_secs(),
    };

    let result = initiator_b.confirm_link(&request_b, &proof_from_a);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Replaying a manual MAC from Session A in Session B must be rejected, got: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// DL-4: Boundary tests (60s/61s)
// ---------------------------------------------------------------------------

// @scenario: device_management :: Proximity proof boundary timing
// @internal
#[test]
fn test_ultrasonic_proof_at_exactly_60_seconds_accepted() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);
    let challenge = initiator.proximity_challenge();

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "Phone".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // Proof exactly at the 60-second boundary
    let proof = ProximityProof::Ultrasonic {
        challenge_response: challenge,
        verified_at: now_unix_secs().saturating_sub(60),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        result.is_ok(),
        "Proof at exactly 60s should be accepted, got: {:?}",
        result.err()
    );
}

// @scenario: device_management :: Proximity proof boundary timing
// @internal
#[test]
fn test_ultrasonic_proof_at_61_seconds_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);
    let challenge = initiator.proximity_challenge();

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "Phone".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // Proof 1 second beyond the boundary
    let proof = ProximityProof::Ultrasonic {
        challenge_response: challenge,
        verified_at: now_unix_secs().saturating_sub(61),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityExpired)),
        "Proof at 61s should be rejected, got: {:?}",
        result.err()
    );
}

// @scenario: device_management :: Proximity proof boundary timing
// @internal
#[test]
fn test_manual_proof_at_exactly_60_seconds_accepted() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "Phone".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();
    let (confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    let proof = ProximityProof::ManualConfirmation {
        confirmation_code_mac: compute_confirmation_mac(
            initiator.qr().link_key(),
            &confirmation.confirmation_code,
        ),
        confirmed_at: now_unix_secs().saturating_sub(60),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        result.is_ok(),
        "Manual proof at exactly 60s should be accepted, got: {:?}",
        result.err()
    );
}

// @scenario: device_management :: Proximity proof boundary timing
// @internal
#[test]
fn test_manual_proof_at_61_seconds_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "Phone".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();
    let (confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    let proof = ProximityProof::ManualConfirmation {
        confirmation_code_mac: compute_confirmation_mac(
            initiator.qr().link_key(),
            &confirmation.confirmation_code,
        ),
        confirmed_at: now_unix_secs().saturating_sub(61),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        matches!(result, Err(ExchangeError::ProximityExpired)),
        "Manual proof at 61s should be rejected, got: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// DL-6: Self-linking prevention tests
// ---------------------------------------------------------------------------

// @scenario: device_management :: Self-linking prevention
// @internal
#[test]
fn test_self_linking_same_device_name_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);
    let challenge = initiator.proximity_challenge();

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();

    // Responder uses the SAME device name as the primary device in the registry
    let mut responder =
        DeviceLinkResponder::from_qr(scanned_qr, "Primary Device".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    let proof = ProximityProof::Ultrasonic {
        challenge_response: challenge,
        verified_at: now_unix_secs(),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        matches!(result, Err(ExchangeError::SelfLinkingNotAllowed)),
        "Linking with same device name as existing device must be rejected, got err: {:?}",
        result.err()
    );
}

// @scenario: device_management :: Self-linking prevention
// @internal
#[test]
fn test_self_linking_restored_initiator_rejected() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
    let qr_string = initiator.qr().to_data_string();

    // Restore initiator from saved QR
    let restored_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let restored_initiator =
        DeviceLinkInitiatorRestored::new(master_seed, &identity, registry, restored_qr);

    let challenge = restored_initiator.proximity_challenge();

    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder =
        DeviceLinkResponder::from_qr(scanned_qr, "Primary Device".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = restored_initiator
        .prepare_confirmation(&encrypted_request)
        .unwrap();

    let proof = ProximityProof::Ultrasonic {
        challenge_response: challenge,
        verified_at: now_unix_secs(),
    };

    let result = restored_initiator.confirm_link(&request, &proof);
    assert!(
        matches!(result, Err(ExchangeError::SelfLinkingNotAllowed)),
        "Restored initiator must also reject self-linking, got err: {:?}",
        result.err()
    );
}

// @scenario: device_management :: Self-linking prevention
// @internal
#[test]
fn test_different_device_name_allowed() {
    // Sanity check: a different device name should succeed
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);
    let challenge = initiator.proximity_challenge();

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();

    // Different device name — this should succeed
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "New Tablet".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    let proof = ProximityProof::Ultrasonic {
        challenge_response: challenge,
        verified_at: now_unix_secs(),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        result.is_ok(),
        "Different device name should be allowed, got: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Property-based tests (proptest)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proximity_proof_proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
            /// Any timestamp older than 60 seconds must be rejected as expired.
    // @internal
            #[test]
            fn test_expired_ultrasonic_timestamp_always_rejected(age in 61u64..=86400u64) {
                let master_seed = [0x42u8; 32];
                let identity = Identity::create("Alice");
                let registry = create_test_registry(&identity);
                let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

                let qr_string = initiator.qr().to_data_string();
                let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
                let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "Phone".to_string()).unwrap();
                let encrypted_request = responder.create_request().unwrap();
                let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

                let proof = ProximityProof::Ultrasonic {
                    challenge_response: initiator.proximity_challenge(),
                    verified_at: now_unix_secs().saturating_sub(age),
                };

                let result = initiator.confirm_link(&request, &proof);
                prop_assert!(
                    matches!(result, Err(ExchangeError::ProximityExpired)),
                    "Expected ProximityExpired for age={}, got err: {:?}", age, result.err()
                );
            }

            /// Any tampered challenge response (different from the real challenge) must be rejected.
    // @internal
            #[test]
            fn test_tampered_challenge_response_always_rejected(tampered in prop::array::uniform16(any::<u8>())) {
                let master_seed = [0x42u8; 32];
                let identity = Identity::create("Alice");
                let registry = create_test_registry(&identity);
                let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

                // Skip if tampered happens to equal the real challenge (astronomically unlikely)
                let real_challenge = initiator.proximity_challenge();
                prop_assume!(tampered != real_challenge);

                let qr_string = initiator.qr().to_data_string();
                let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
                let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "Phone".to_string()).unwrap();
                let encrypted_request = responder.create_request().unwrap();
                let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

                let proof = ProximityProof::Ultrasonic {
                    challenge_response: tampered,
                    verified_at: now_unix_secs(),
                };

                let result = initiator.confirm_link(&request, &proof);
                prop_assert!(
                    matches!(result, Err(ExchangeError::ProximityNotVerified)),
                    "Expected ProximityNotVerified for tampered challenge, got err: {:?}", result.err()
                );
            }

            /// Any tampered MAC (different from the real one) must be rejected.
    // @internal
            #[test]
            fn test_tampered_confirmation_mac_always_rejected(tampered_mac in prop::array::uniform32(any::<u8>())) {
                let master_seed = [0x42u8; 32];
                let identity = Identity::create("Alice");
                let registry = create_test_registry(&identity);
                let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

                let qr_string = initiator.qr().to_data_string();
                let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
                let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "Phone".to_string()).unwrap();
                let encrypted_request = responder.create_request().unwrap();
                let (confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

                // Skip if tampered MAC happens to equal the real MAC
                let real_mac = compute_confirmation_mac(initiator.qr().link_key(), &confirmation.confirmation_code);
                prop_assume!(tampered_mac != real_mac);

                let proof = ProximityProof::ManualConfirmation {
                    confirmation_code_mac: tampered_mac,
                    confirmed_at: now_unix_secs(),
                };

                let result = initiator.confirm_link(&request, &proof);
                prop_assert!(
                    matches!(result, Err(ExchangeError::ProximityNotVerified)),
                    "Expected ProximityNotVerified for tampered MAC, got err: {:?}", result.err()
                );
            }
        }
}
