// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for device linking proximity verification.
//!
//! Verifies that `build_response()` refuses to release the master seed
//! unless proximity has been verified, and that the proximity challenge
//! derivation is deterministic and consistent across both sides.

use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkInitiatorRestored, DeviceLinkQR, DeviceLinkResponder,
    ExchangeError,
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

#[test]
fn test_build_response_rejected_without_proximity() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    // Initiator without proximity verification
    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();

    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // confirm_link() should fail because proximity was never verified
    let result = initiator.confirm_link(&request);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Expected ProximityNotVerified, got: {:?}",
        result.err()
    );
}

#[test]
fn test_build_response_succeeds_after_proximity_verified() {
    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let registry = create_test_registry(&identity);

    let mut initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();

    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    // Set proximity verified, then confirm should succeed
    initiator.set_proximity_verified();
    let (encrypted_response, updated_registry, new_device) =
        initiator.confirm_link(&request).unwrap();

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

    // Should fail without proximity verification
    let result = restored_initiator.confirm_link(&request);
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

    // Initiator without proximity verification
    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();
    let encrypted_request = responder.create_request().unwrap();

    // Deprecated process_request() should also enforce proximity
    let result = initiator.process_request(&encrypted_request);
    assert!(
        matches!(result, Err(ExchangeError::ProximityNotVerified)),
        "Expected ProximityNotVerified on deprecated API, got: {:?}",
        result.err()
    );
}
