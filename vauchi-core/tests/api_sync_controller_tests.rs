//! Tests for api::sync_controller
//! Extracted from sync_controller.rs

use std::sync::Arc;
use vauchi_core::api::*;
use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::network::{MockTransport, RelayClientConfig, TransportConfig};
use vauchi_core::*;

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

fn create_test_relay() -> RelayClient<MockTransport> {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
    };
    RelayClient::new(transport, config, "test-identity".into())
}

fn create_test_ratchet() -> DoubleRatchetState {
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key())
}

#[test]
fn test_sync_controller_connect_disconnect() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    assert!(!controller.is_connected());

    controller.connect().unwrap();
    assert!(controller.is_connected());

    controller.disconnect().unwrap();
    assert!(!controller.is_connected());
}

#[test]
fn test_sync_controller_ratchet_management() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    let ratchet = create_test_ratchet();
    controller.register_ratchet("contact-1", ratchet);

    assert!(controller.has_ratchet("contact-1"));
    assert!(!controller.has_ratchet("contact-2"));

    let removed = controller.remove_ratchet("contact-1");
    assert!(removed.is_some());
    assert!(!controller.has_ratchet("contact-1"));
}

#[test]
fn test_sync_controller_sync_not_connected() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    // Should fail when not connected
    let result = controller.sync();
    assert!(matches!(result, Err(VauchiError::Network(_))));
}

#[test]
fn test_sync_controller_sync_empty() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller.connect().unwrap();

    // Sync with no pending updates
    let result = controller.sync().unwrap();
    assert_eq!(result.sent, 0);
    assert_eq!(result.acknowledged, 0);
    assert_eq!(result.failed, 0);
}

#[test]
fn test_sync_controller_get_sync_state() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SyncController::new(relay, &storage, config, events);

    // No pending updates = synced
    let state = controller.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, SyncState::Synced { .. }));
}

#[test]
fn test_sync_controller_pending_count() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SyncController::new(relay, &storage, config, events);

    // Initially no pending
    assert_eq!(controller.pending_count().unwrap(), 0);
}

#[test]
fn test_sync_controller_in_flight_count() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SyncController::new(relay, &storage, config, events);

    // Initially no in-flight
    assert_eq!(controller.in_flight_count(), 0);
}

#[test]
fn test_sync_controller_auto_sync_config() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());

    // Test with auto_sync enabled
    let config = SyncConfig {
        auto_sync: true,
        ..Default::default()
    };
    let controller = SyncController::new(relay, &storage, config, events.clone());
    assert!(controller.is_auto_sync_enabled());

    // Test with auto_sync disabled
    let relay2 = create_test_relay();
    let config2 = SyncConfig {
        auto_sync: false,
        ..Default::default()
    };
    let controller2 = SyncController::new(relay2, &storage, config2, events);
    assert!(!controller2.is_auto_sync_enabled());
}

#[test]
fn test_sync_result_default() {
    let result = SyncResult::default();
    assert_eq!(result.sent, 0);
    assert_eq!(result.acknowledged, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(result.timed_out, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn test_sync_controller_sync_contact_no_ratchet() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller.connect().unwrap();

    // Should fail with no ratchet
    let result = controller.sync_contact("contact-1");
    assert!(matches!(result, Err(VauchiError::InvalidState(_))));
}

#[test]
fn test_sync_controller_sync_contact_with_ratchet() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller.connect().unwrap();

    // Register ratchet
    let ratchet = create_test_ratchet();
    controller.register_ratchet("contact-1", ratchet);

    // Should succeed (no pending updates)
    let result = controller.sync_contact("contact-1").unwrap();
    assert_eq!(result.sent, 0);
}

// ============================================================
// Phase 7: Device Sync Integration Tests (TDD)
// ============================================================

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::device::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::{DeviceSyncOrchestrator, SyncItem};

fn create_test_device(master_seed: &[u8; 32], index: u32, name: &str) -> DeviceInfo {
    DeviceInfo::derive(master_seed, index, name.to_string())
}

fn create_test_registry(master_seed: &[u8; 32], device: &DeviceInfo) -> DeviceRegistry {
    let signing_key = SigningKeyPair::from_seed(master_seed);
    DeviceRegistry::new(device.to_registered(master_seed), &signing_key)
}

#[test]
fn test_sync_controller_send_device_sync() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller.connect().unwrap();

    // Create device orchestrator
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();
    let device_b_public_key = *device_b.exchange_public_key();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Record a local change
    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    // Send device sync via controller
    let result = controller.send_device_sync(&orchestrator, &device_b_id, &device_b_public_key);
    assert!(result.is_ok());
}

#[test]
fn test_sync_controller_process_device_sync() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SyncController::new(relay, &storage, config, events);

    // Create device orchestrator
    let master_seed = [0x42u8; 32];
    let device = create_test_device(&master_seed, 0, "Test Device");
    let registry = create_test_registry(&master_seed, &device);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device, registry);

    // Create incoming sync items
    let incoming = vec![SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+1234567890".to_string(),
        timestamp: 1000,
    }];

    // Process via controller
    let applied = controller.process_device_sync(&mut orchestrator, incoming);
    assert!(applied.is_ok());
    assert_eq!(applied.unwrap().len(), 1);
}
