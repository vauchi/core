// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device Linking Protocol
//!
//! Enables linking multiple devices to the same identity via QR code scanning.
//! The existing device generates a QR containing a link key, the new device
//! scans it and receives the encrypted master seed to derive identical keys.

mod initiator;
mod qr;
mod request;
mod responder;
mod response;
mod types;

pub use initiator::{DeviceLinkInitiator, DeviceLinkInitiatorRestored};
pub use qr::DeviceLinkQR;
pub use request::DeviceLinkRequest;
pub use responder::DeviceLinkResponder;
pub use response::DeviceLinkResponse;
pub use types::{
    DeviceLinkConfirmation, ProximityProof, compute_confirmation_mac, generate_numeric_code,
};

// INLINE_TEST_REQUIRED: Tests private DEVICE_LINK_VERSION, DEVICE_LINK_MAGIC, BASE64 constants and version field
#[cfg(test)]
mod tests {
    use super::*;

    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    use super::types::{DEVICE_LINK_MAGIC, DEVICE_LINK_VERSION};
    use crate::crypto::SymmetricKey;
    use crate::identity::{DeviceInfo, DeviceRegistry, Identity};

    use super::super::ExchangeError;

    fn create_test_identity() -> Identity {
        Identity::create("Test User")
    }

    fn create_test_registry(identity: &Identity) -> DeviceRegistry {
        let device_info = identity.device_info();
        let master_seed = [0x42u8; 32]; // Test seed
        DeviceRegistry::new(
            device_info.to_registered(&master_seed),
            identity.signing_keypair(),
        )
    }

    /// Creates a valid ultrasonic proximity proof for testing.
    fn create_valid_proof(initiator_challenge: [u8; 16]) -> ProximityProof {
        ProximityProof::Ultrasonic {
            challenge_response: initiator_challenge,
            verified_at: crate::exchange::now_secs(),
        }
    }

    #[test]
    fn test_device_link_qr_generation() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        assert_eq!(qr.version, DEVICE_LINK_VERSION);
        assert_eq!(qr.identity_public_key(), identity.signing_public_key());
        assert!(!qr.is_expired());
    }

    #[test]
    fn test_device_link_qr_signature_valid() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        assert!(qr.verify_signature());
    }

    #[test]
    fn test_device_link_qr_roundtrip() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        let data_string = qr.to_data_string();
        let restored = DeviceLinkQR::from_data_string(&data_string).unwrap();

        assert_eq!(restored.identity_public_key(), qr.identity_public_key());
        assert_eq!(restored.link_key(), qr.link_key());
        assert_eq!(restored.timestamp(), qr.timestamp());
    }

    #[test]
    fn test_device_link_qr_expired() {
        let identity = create_test_identity();
        // Create QR with timestamp 20 minutes ago
        let old_timestamp = crate::exchange::now_secs() - 1200;

        let qr = DeviceLinkQR::generate_with_timestamp(&identity, old_timestamp);
        assert!(qr.is_expired());
    }

    #[test]
    fn test_device_link_request_roundtrip() {
        let request = DeviceLinkRequest::new("My New Phone".to_string());
        let bytes = request.to_bytes();
        let restored = DeviceLinkRequest::from_bytes(&bytes).unwrap();

        assert_eq!(restored.device_name, request.device_name);
        assert_eq!(restored.nonce, request.nonce);
        assert_eq!(restored.timestamp, request.timestamp);
    }

    #[test]
    fn test_device_link_request_encryption() {
        let request = DeviceLinkRequest::new("My New Phone".to_string());
        let link_key = [0x42u8; 32];

        let encrypted = request.encrypt(&link_key).unwrap();
        let decrypted = DeviceLinkRequest::decrypt(&encrypted, &link_key).unwrap();

        assert_eq!(decrypted.device_name, request.device_name);
    }

    #[test]
    fn test_device_link_response_roundtrip() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let response =
            DeviceLinkResponse::new(master_seed, "Alice".to_string(), 1, registry.clone());

        let bytes = response.to_bytes();
        let restored = DeviceLinkResponse::from_bytes(&bytes).unwrap();

        assert_eq!(restored.master_seed(), &master_seed);
        assert_eq!(restored.display_name(), "Alice");
        assert_eq!(restored.device_index(), 1);
    }

    #[test]
    fn test_device_link_response_encryption() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let response = DeviceLinkResponse::new(master_seed, "Alice".to_string(), 1, registry);

        let link_key = [0x55u8; 32];
        let encrypted = response.encrypt(&link_key).unwrap();
        let decrypted = DeviceLinkResponse::decrypt(&encrypted, &link_key).unwrap();

        assert_eq!(decrypted.master_seed(), &master_seed);
        assert_eq!(decrypted.display_name(), "Alice");
        assert_eq!(decrypted.device_index(), 1);
    }

    #[test]
    #[allow(deprecated)]
    fn test_device_link_full_flow() {
        // Existing device (Device A) setup
        let master_seed_a = [0x42u8; 32];
        let identity_a = Identity::create("Alice");
        let registry_a = create_test_registry(&identity_a);

        // Device A creates link initiator
        let initiator = DeviceLinkInitiator::new(master_seed_a, &identity_a, registry_a);
        let proof = create_valid_proof(initiator.proximity_challenge());
        let qr = initiator.qr();

        // New device (Device B) scans QR
        let qr_string = qr.to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        // Device B creates request
        let encrypted_request = responder.create_request().unwrap();

        // Device A processes request and creates response
        let (encrypted_response, updated_registry, new_device) = initiator
            .process_request(&encrypted_request, &proof)
            .unwrap();

        // Device B processes response
        let response = responder.process_response(&encrypted_response).unwrap();

        // Verify the new device got the correct seed
        assert_eq!(response.master_seed(), &master_seed_a);
        assert_eq!(response.display_name(), "Alice");
        assert_eq!(response.device_index(), 1); // Second device gets index 1

        // Verify the new device info is correct
        assert_eq!(new_device.device_name(), "My Phone");
        assert_eq!(new_device.device_index(), 1);

        // Verify the registry was updated
        assert_eq!(updated_registry.device_count(), 2);
    }

    #[test]
    fn test_device_link_qr_wrong_magic() {
        let data = BASE64.encode(b"XXXX01\x00\x00\x00");
        let result = DeviceLinkQR::from_data_string(&data);
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_request_wrong_key() {
        let request = DeviceLinkRequest::new("My Phone".to_string());
        let correct_key = [0x42u8; 32];
        let wrong_key = [0x99u8; 32];

        let encrypted = request.encrypt(&correct_key).unwrap();
        let result = DeviceLinkRequest::decrypt(&encrypted, &wrong_key);

        assert!(result.is_err(), "expected error");
    }

    // ============================================================
    // Additional edge case tests (added for coverage)
    // ============================================================

    #[test]
    fn test_device_link_response_with_sync_payload() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let sync_payload = r#"{"contacts":[],"own_card_json":"{}","version":1}"#;
        let response = DeviceLinkResponse::with_sync_payload(
            master_seed,
            "Alice".to_string(),
            1,
            registry.clone(),
            sync_payload.to_string(),
        );

        assert_eq!(response.sync_payload_json(), sync_payload);

        // Test roundtrip preserves sync payload
        let bytes = response.to_bytes();
        let restored = DeviceLinkResponse::from_bytes(&bytes).unwrap();
        assert_eq!(restored.sync_payload_json(), sync_payload);
    }

    #[test]
    fn test_device_link_response_encryption_with_sync_payload() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let sync_payload = r#"{"contacts":[{"id":"test"}]}"#;
        let response = DeviceLinkResponse::with_sync_payload(
            master_seed,
            "Alice".to_string(),
            1,
            registry,
            sync_payload.to_string(),
        );

        let link_key = [0x55u8; 32];
        let encrypted = response.encrypt(&link_key).unwrap();
        let decrypted = DeviceLinkResponse::decrypt(&encrypted, &link_key).unwrap();

        assert_eq!(decrypted.sync_payload_json(), sync_payload);
    }

    #[test]
    fn test_device_link_responder_expired_qr() {
        let identity = create_test_identity();
        // Create QR with timestamp 20 minutes ago (expired)
        let old_timestamp = crate::exchange::now_secs() - 1200;

        let qr = DeviceLinkQR::generate_with_timestamp(&identity, old_timestamp);
        let result = DeviceLinkResponder::from_qr(qr, "My Phone".to_string());

        assert!(matches!(result, Err(ExchangeError::TokenExpired)));
    }

    #[test]
    fn test_device_link_qr_invalid_base64() {
        let result = DeviceLinkQR::from_data_string("not valid base64!!!");
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_qr_invalid_version() {
        // Create valid-looking data but with wrong version
        let mut data = Vec::new();
        data.extend_from_slice(DEVICE_LINK_MAGIC);
        data.push(99); // Wrong version
        data.extend_from_slice(&[0u8; 32]); // identity_key
        data.extend_from_slice(&[0u8; 32]); // link_key
        data.extend_from_slice(&0u64.to_be_bytes()); // timestamp
        data.extend_from_slice(&[0u8; 64]); // signature

        let encoded = BASE64.encode(&data);
        let result = DeviceLinkQR::from_data_string(&encoded);

        assert!(matches!(result, Err(ExchangeError::InvalidProtocolVersion)));
    }

    #[test]
    fn test_device_link_qr_truncated_data() {
        // Data too short
        let data = BASE64.encode(b"WBDL\x01short");
        let result = DeviceLinkQR::from_data_string(&data);

        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_qr_invalid_signature() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        // Corrupt the signature
        let mut data = Vec::new();
        data.extend_from_slice(DEVICE_LINK_MAGIC);
        data.push(qr.version);
        data.extend_from_slice(qr.identity_public_key());
        data.extend_from_slice(qr.link_key());
        data.extend_from_slice(&qr.timestamp().to_be_bytes());
        data.extend_from_slice(&[0xFFu8; 64]); // Invalid signature

        let encoded = BASE64.encode(&data);
        let result = DeviceLinkQR::from_data_string(&encoded);

        assert!(matches!(result, Err(ExchangeError::InvalidSignature)));
    }

    #[test]
    #[allow(deprecated)]
    fn test_device_link_process_request_empty_device_name() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);
        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);
        let proof = create_valid_proof(initiator.proximity_challenge());

        // Create a request with empty device name
        let request = DeviceLinkRequest {
            device_name: "".to_string(),
            nonce: [0u8; 32],
            timestamp: crate::exchange::now_secs(),
        };
        let encrypted = request.encrypt(initiator.qr().link_key()).unwrap();

        let result = initiator.process_request(&encrypted, &proof);
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_request_truncated_bytes() {
        // Test with truncated data
        let result = DeviceLinkRequest::from_bytes(&[0u8; 10]);
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_response_truncated_bytes() {
        // Test with truncated data
        let result = DeviceLinkResponse::from_bytes(&[0u8; 10]);
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_response_wrong_key() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let response = DeviceLinkResponse::new(master_seed, "Alice".to_string(), 1, registry);

        let correct_key = [0x42u8; 32];
        let wrong_key = [0x99u8; 32];

        let encrypted = response.encrypt(&correct_key).unwrap();
        let result = DeviceLinkResponse::decrypt(&encrypted, &wrong_key);

        assert!(result.is_err(), "expected error");
    }

    #[test]
    fn test_device_link_qr_to_qr_image_string() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        let image_string = qr.to_qr_image_string();

        // Should produce a non-empty string with blocks
        assert!(!image_string.is_empty());
        assert!(image_string.contains('█') || image_string.contains(' '));
    }

    #[test]
    fn test_device_link_responder_identity_public_key() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);
        let responder = DeviceLinkResponder::from_qr(qr, "My Phone".to_string()).unwrap();

        assert_eq!(
            responder.identity_public_key(),
            identity.signing_public_key()
        );
    }

    #[test]
    fn test_device_link_initiator_qr_accessor() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);
        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

        let qr = initiator.qr();
        assert_eq!(qr.identity_public_key(), identity.signing_public_key());
        assert!(!qr.is_expired());
    }

    // ============================================================
    // Phase 8: Device Linking with Sync Payload Tests (TDD)
    // ============================================================

    use crate::contact::Contact;
    use crate::contact_card::ContactCard;
    use crate::storage::Storage;
    use crate::sync::{DeviceSyncOrchestrator, DeviceSyncPayload};

    fn create_test_storage() -> Storage {
        Storage::in_memory(SymmetricKey::generate()).unwrap()
    }

    fn create_test_contact(name: &str) -> Contact {
        let public_key = [0x42u8; 32];
        let card = ContactCard::new(name);
        let shared_key = SymmetricKey::generate();
        Contact::from_exchange(public_key, card, shared_key)
    }

    #[test]
    #[allow(deprecated)]
    fn test_device_link_with_full_sync_payload() {
        // Existing device (Device A) setup with data
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);
        let storage = create_test_storage();

        // Add some data to sync
        let contact = create_test_contact("Bob");
        storage.save_contact(&contact).unwrap();

        let mut own_card = ContactCard::new("Alice");
        let _ = own_card.add_field(crate::contact_card::ContactField::new(
            crate::contact_card::FieldType::Email,
            "email",
            "alice@example.com",
            crate::clock::ambient_now_secs(),
        ));
        storage.save_own_card(&own_card).unwrap();

        // Create orchestrator to generate sync payload
        let device_a = DeviceInfo::derive(&master_seed, 0, "Device A".to_string());
        let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry.clone());
        let sync_payload = orchestrator.create_full_sync_payload().unwrap();
        let sync_json = serde_json::to_string(&sync_payload).unwrap();

        // Create initiator with sync payload
        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
        let proof = create_valid_proof(initiator.proximity_challenge());

        // New device scans QR
        let qr_string = initiator.qr().to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        // Device B creates request
        let encrypted_request = responder.create_request().unwrap();

        // Device A processes request with sync payload
        let (encrypted_response, _updated_registry, _new_device) = initiator
            .process_request_with_sync(&encrypted_request, &sync_json, &proof)
            .unwrap();

        // Device B processes response
        let response = responder.process_response(&encrypted_response).unwrap();

        // Verify sync payload is included
        assert!(!response.sync_payload_json().is_empty());

        // Parse and verify sync payload contents
        let received_payload: DeviceSyncPayload =
            serde_json::from_str(response.sync_payload_json()).unwrap();
        assert_eq!(received_payload.contact_count(), 1);
        assert!(!received_payload.own_card_json.is_empty());
    }

    #[test]
    fn test_new_device_applies_full_state() {
        // Create sync payload
        let contact = create_test_contact("Bob");
        let own_card = ContactCard::new("Alice");
        let payload = DeviceSyncPayload::new(&[contact], &own_card, 1);
        let payload_json = serde_json::to_string(&payload).unwrap();

        // New device receives and parses payload
        let received: DeviceSyncPayload = serde_json::from_str(&payload_json).unwrap();

        // Verify payload contents
        assert_eq!(received.contact_count(), 1);
        assert_eq!(received.version, 1);
    }

    #[test]
    #[allow(deprecated)]
    fn test_device_link_initiator_restored_flow() {
        // Device A creates a QR and saves it
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
        let qr = initiator.qr();
        let qr_string = qr.to_data_string();

        // Later, Device A restores the QR from saved string
        let restored_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let restored_initiator =
            DeviceLinkInitiatorRestored::new(master_seed, &identity, registry, restored_qr);
        let proof = create_valid_proof(restored_initiator.proximity_challenge());

        // Device B scans the QR and creates request
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();
        let encrypted_request = responder.create_request().unwrap();

        // Device A processes request using restored initiator
        let (encrypted_response, updated_registry, new_device) = restored_initiator
            .process_request(&encrypted_request, &proof)
            .unwrap();

        // Device B processes response
        let response = responder.process_response(&encrypted_response).unwrap();

        // Verify the flow worked correctly
        assert_eq!(response.master_seed(), &master_seed);
        assert_eq!(response.display_name(), "Alice");
        assert_eq!(response.device_index(), 1);
        assert_eq!(new_device.device_name(), "My Phone");
        assert_eq!(updated_registry.device_count(), 2);
    }

    #[test]
    fn test_identity_device_link_helper_methods() {
        // Test the new Identity helper methods
        let identity = Identity::create("Alice");

        // Test initial_device_registry
        let registry = identity.initial_device_registry();
        assert_eq!(registry.device_count(), 1);

        // Test create_device_link_initiator
        let initiator = identity.create_device_link_initiator(registry.clone());
        assert!(!initiator.qr().is_expired());
        assert_eq!(
            initiator.qr().identity_public_key(),
            identity.signing_public_key()
        );

        // Test restore_device_link_initiator
        let qr_string = initiator.qr().to_data_string();
        let restored_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let _restored = identity.restore_device_link_initiator(registry, restored_qr);
    }

    // ============================================================
    // Two-Phase Confirmation Flow Tests
    // ============================================================

    #[test]
    fn test_confirmation_code_matches_both_sides() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

        let qr_string = initiator.qr().to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        let encrypted_request = responder.create_request().unwrap();

        // Initiator prepares confirmation
        let (confirmation, _request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

        // Responder computes confirmation code
        let responder_code = responder.compute_confirmation_code().unwrap();

        // Both sides should show the same code
        assert_eq!(confirmation.confirmation_code, responder_code);
        assert_eq!(confirmation.device_name, "My Phone");

        // Code should be in XXX-XXX format
        assert_eq!(confirmation.confirmation_code.len(), 7);
        assert_eq!(&confirmation.confirmation_code[3..4], "-");
    }

    #[test]
    fn test_prepare_and_confirm_flow() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

        let qr_string = initiator.qr().to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        let encrypted_request = responder.create_request().unwrap();

        // Phase 1: Prepare confirmation
        let (confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();
        assert_eq!(confirmation.device_name, "My Phone");

        // Phase 2: Proximity verified, user confirms, complete the link
        let proof = create_valid_proof(initiator.proximity_challenge());
        let (encrypted_response, updated_registry, new_device) =
            initiator.confirm_link(&request, &proof).unwrap();

        // Device B processes response
        let response = responder.process_response(&encrypted_response).unwrap();

        assert_eq!(response.master_seed(), &master_seed);
        assert_eq!(response.display_name(), "Alice");
        assert_eq!(response.device_index(), 1);
        assert_eq!(new_device.device_name(), "My Phone");
        assert_eq!(updated_registry.device_count(), 2);
    }

    #[test]
    fn test_prepare_and_confirm_with_sync_flow() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);
        let storage = create_test_storage();

        let contact = create_test_contact("Bob");
        storage.save_contact(&contact).unwrap();

        let device_a = DeviceInfo::derive(&master_seed, 0, "Device A".to_string());
        let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry.clone());
        let sync_payload = orchestrator.create_full_sync_payload().unwrap();
        let sync_json = serde_json::to_string(&sync_payload).unwrap();

        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

        let qr_string = initiator.qr().to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        let encrypted_request = responder.create_request().unwrap();

        let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

        let proof = create_valid_proof(initiator.proximity_challenge());
        let (encrypted_response, _updated_registry, _new_device) = initiator
            .confirm_link_with_sync(&request, &sync_json, &proof)
            .unwrap();

        let response = responder.process_response(&encrypted_response).unwrap();
        assert!(!response.sync_payload_json().is_empty());

        let received_payload: DeviceSyncPayload =
            serde_json::from_str(response.sync_payload_json()).unwrap();
        assert_eq!(received_payload.contact_count(), 1);
    }

    #[test]
    fn test_confirmation_code_without_create_request_fails() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);
        let responder = DeviceLinkResponder::from_qr(qr, "My Phone".to_string()).unwrap();

        // Should fail because create_request() was never called
        let result = responder.compute_confirmation_code();
        assert!(result.is_err(), "expected error");
    }

    #[test]
    fn test_identity_fingerprint_format() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        let fingerprint = qr.identity_fingerprint();

        // Format: XXXX-XXXX-XXXX-XXXX (4 groups of 4 hex chars)
        let parts: Vec<&str> = fingerprint.split('-').collect();
        assert_eq!(
            parts.len(),
            4,
            "Fingerprint should have 4 groups: {}",
            fingerprint
        );
        for part in &parts {
            assert_eq!(part.len(), 4, "Each group should be 4 hex chars: {}", part);
            assert!(part.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn test_identity_fingerprint_matches_both_sides() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

        let qr_string = initiator.qr().to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        let encrypted_request = responder.create_request().unwrap();

        let (confirmation, _request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

        let responder_fingerprint = responder.identity_fingerprint();

        assert_eq!(confirmation.identity_fingerprint, responder_fingerprint);
    }

    #[test]
    #[allow(deprecated)]
    fn test_deprecated_process_request_still_works() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);
        let proof = create_valid_proof(initiator.proximity_challenge());

        let qr_string = initiator.qr().to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        let encrypted_request = responder.create_request().unwrap();

        // Old API should still work (with valid proximity proof)
        let (encrypted_response, updated_registry, new_device) = initiator
            .process_request(&encrypted_request, &proof)
            .unwrap();

        let response = responder.process_response(&encrypted_response).unwrap();

        assert_eq!(response.master_seed(), &master_seed);
        assert_eq!(new_device.device_name(), "My Phone");
        assert_eq!(updated_registry.device_count(), 2);
    }

    #[test]
    fn test_restored_initiator_confirmation_flow() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
        let qr_string = initiator.qr().to_data_string();

        let restored_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let restored_initiator =
            DeviceLinkInitiatorRestored::new(master_seed, &identity, registry, restored_qr);

        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        let encrypted_request = responder.create_request().unwrap();

        // Prepare confirmation on restored initiator
        let (confirmation, request) = restored_initiator
            .prepare_confirmation(&encrypted_request)
            .unwrap();

        // Codes should match
        let responder_code = responder.compute_confirmation_code().unwrap();
        assert_eq!(confirmation.confirmation_code, responder_code);

        // Proximity verified, confirm and complete
        let proof = create_valid_proof(restored_initiator.proximity_challenge());
        let (encrypted_response, updated_registry, new_device) =
            restored_initiator.confirm_link(&request, &proof).unwrap();

        let response = responder.process_response(&encrypted_response).unwrap();
        assert_eq!(response.master_seed(), &master_seed);
        assert_eq!(new_device.device_name(), "My Phone");
        assert_eq!(updated_registry.device_count(), 2);
    }
}
