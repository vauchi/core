// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
        ..Default::default()
    };
    RelayClient::new(transport, config, "test-identity".into())
}

fn create_test_ratchet() -> DoubleRatchetState {
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap()
}

// @internal
#[test]
fn test_sync_controller_connect_disconnect() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    assert!(!controller.is_connected());

    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert!(controller.is_connected());

    controller
        .disconnect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert!(!controller.is_connected());
}

// @internal
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
    assert!(removed.is_some(), "expected Some value");
    assert!(!controller.has_ratchet("contact-1"));
}

// @internal
#[test]
fn test_sync_controller_sync_not_connected() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    // Should fail when not connected
    let result = controller.sync(&vauchi_core::rng::OsSecureRng::new());
    assert!(matches!(result, Err(VauchiError::Network(_))));
}

// @internal
#[test]
fn test_sync_controller_sync_empty() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    // Sync with no pending updates
    let result = controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert_eq!(result.sent, 0);
    assert_eq!(result.acknowledged, 0);
    assert_eq!(result.failed, 0);
}

// @internal
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

// @internal
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

// @internal
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

// @internal
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

// @internal
#[test]
fn test_sync_result_default() {
    let result = SyncResult::default();
    assert_eq!(result.sent, 0);
    assert_eq!(result.acknowledged, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(result.timed_out, 0);
    assert!(result.errors.is_empty());
}

// @scenario: sync_updates :: Sync result tracks operation totals
// @internal
#[test]
fn test_sync_result_total() {
    let result = SyncResult {
        sent: 3,
        acknowledged: 2,
        failed: 1,
        timed_out: 1,
        ..Default::default()
    };
    assert_eq!(result.total(), 7);
}

// @scenario: sync_updates :: Sync result detects changes
// @internal
#[test]
fn test_sync_result_has_changes() {
    let mut result = SyncResult::default();
    assert!(!result.has_changes(), "Default should have no changes");

    result.sent = 1;
    assert!(result.has_changes(), "Sent > 0 means changes");

    result.sent = 0;
    result.acknowledged = 1;
    assert!(result.has_changes(), "Acknowledged > 0 means changes");

    result.acknowledged = 0;
    result.failed = 5;
    assert!(
        !result.has_changes(),
        "Only failed/timed_out don't count as changes"
    );
}

// @scenario: sync_updates :: Sync result total with all zeros
// @internal
#[test]
fn test_sync_result_total_empty() {
    let result = SyncResult::default();
    assert_eq!(result.total(), 0);
    assert!(!result.has_changes());
}

// @scenario: sync_updates :: Batch size limiting
// @internal
#[test]
fn test_sync_controller_batch_size_config() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());

    let config = SyncConfig {
        batch_size: Some(5),
        ..Default::default()
    };

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    // Sync with batch_size=5 and no pending updates
    let result = controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert_eq!(result.sent, 0);
    assert_eq!(result.total(), 0);
}

// @scenario: sync_updates :: Sync contact not connected
// @internal
#[test]
fn test_sync_contact_not_connected() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    // Not connected — should fail
    let result = controller.sync_contact("contact-1");
    assert!(matches!(result, Err(VauchiError::Network(_))));
}

// @scenario: sync_updates :: Connection state tracking
// @internal
#[test]
fn test_sync_controller_connection_state() {
    use vauchi_core::network::ConnectionState;

    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    // Initially disconnected
    assert_eq!(controller.connection_state(), ConnectionState::Disconnected);

    // After connect
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert_eq!(controller.connection_state(), ConnectionState::Connected);

    // After disconnect
    controller
        .disconnect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert_eq!(controller.connection_state(), ConnectionState::Disconnected);
}

// @scenario: sync_updates :: Sync status across contacts
// @internal
#[test]
fn test_sync_controller_sync_status() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SyncController::new(relay, &storage, config, events);

    // Should return empty map when no contacts
    let status = controller.sync_status().unwrap();
    assert!(status.is_empty());
}

// @scenario: sync_updates :: Relay accessor methods
// @internal
#[test]
fn test_sync_controller_relay_accessors() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    // Read-only relay access
    let _relay = controller.relay();
    assert!(!controller.is_connected());

    // Mutable relay access
    let _relay_mut = controller.relay_mut();

    // Sync manager access
    let _sm = controller.sync_manager();

    // Retry scheduler access
    let _rs = controller.retry_scheduler();

    // Offline manager access
    let _om = controller.offline_manager();
}

// @scenario: sync_updates :: Ratchet removal for non-existent contact
// @internal
#[test]
fn test_sync_controller_remove_nonexistent_ratchet() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);

    let removed = controller.remove_ratchet("nonexistent");
    assert!(
        removed.is_none(),
        "Removing non-existent ratchet should return None"
    );
}

// @internal
#[test]
fn test_sync_controller_sync_contact_no_ratchet() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    // Should fail with no ratchet
    let result = controller.sync_contact("contact-1");
    assert!(matches!(result, Err(VauchiError::InvalidState(_))));
}

// @internal
#[test]
fn test_sync_controller_sync_contact_with_ratchet() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    // Save a contact with a shared_key so sync_contact can derive a
    // daily-rotating mailbox token. (Pre-2026-05-23 the test relied on
    // the implicit ADR-029 violation that let sync_contact succeed for
    // contacts missing from storage — see
    // `sync_contact_errors_when_load_contact_returns_none_adr029`.)
    let public_key = [0xA1u8; 32];
    let card = vauchi_core::contact_card::ContactCard::new("Alice");
    let shared_key = SymmetricKey::generate();
    let contact = vauchi_core::contact::Contact::from_exchange(public_key, card, shared_key, 0);
    let contact_id = contact.id().to_string();
    storage.save_contact(&contact).unwrap();

    // Register ratchet under the same contact_id.
    let ratchet = create_test_ratchet();
    controller.register_ratchet(&contact_id, ratchet);

    // Should succeed (no pending updates).
    let result = controller.sync_contact(&contact_id).unwrap();
    assert_eq!(result.sent, 0);
}

// ============================================================
// Phase 7: Device Sync Integration Tests (TDD)
// ============================================================

use vauchi_core::DeviceSyncOrchestrator;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::device::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::SyncItem;

fn create_test_device(master_seed: &[u8; 32], index: u32, name: &str) -> DeviceInfo {
    DeviceInfo::derive(master_seed, index, name.to_string(), 0)
}

fn create_test_registry(master_seed: &[u8; 32], device: &DeviceInfo) -> DeviceRegistry {
    let signing_key = SigningKeyPair::from_seed(master_seed);
    DeviceRegistry::new(device.to_registered(master_seed), &signing_key)
}

// test_sync_controller_send_device_sync removed (SP-33): send_device_sync
// reimplemented via EncryptedUpdate + self-token (Task 4.3 done).

// @internal
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
    let applied = controller.process_device_sync(&mut orchestrator, incoming, &[0x42u8; 32]);
    assert!(applied.is_ok(), "expected success");
    assert_eq!(applied.unwrap().len(), 1);
}

// ============================================================
// ADR-029 stable-token-fallback regression
// (_private/.../2026-05-21-silent-failures-in-security-paths
//  site 5: sync_controller mailbox-token fallback)
// ============================================================

use vauchi_core::{PendingUpdate, UpdateStatus};

/// When `load_contact` returns `Ok(None)` (contact missing from storage),
/// `sync()` must NOT fall back to using the plaintext `contact_id` as the
/// recipient_id — that would reintroduce a stable token (ADR-029
/// violation). The update is skipped and recorded as failed instead.
// @internal
#[test]
fn sync_skips_update_when_load_contact_returns_none_adr029() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    // Ratchet registered, but contact NOT saved to storage —
    // load_contact will return Ok(None).
    let ratchet = create_test_ratchet();
    controller.register_ratchet("contact-adr029", ratchet);

    let update = PendingUpdate {
        id: "u-adr029-1".to_string(),
        contact_id: "contact-adr029".to_string(),
        update_type: "card_update".to_string(),
        payload: vec![1, 2, 3],
        created_at: 0,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
    };
    storage
        .queue_update(&update)
        .expect("queue_update should succeed");

    let result = controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .expect("sync should return Ok (skipping is not an error at this layer)");

    assert_eq!(
        result.sent, 0,
        "must not send any update without a derivable mailbox token"
    );
    assert_eq!(
        result.failed, 1,
        "missing-shared-key skip must be recorded as failed"
    );
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].0, "contact-adr029");
    let msg = &result.errors[0].1;
    assert!(
        msg.contains("mailbox token")
            || msg.contains("shared_key")
            || msg.contains("shared key")
            || msg.contains("not found"),
        "error message should indicate token-derivation failure: got {msg}"
    );
}

/// `sync_contact()` takes a specific contact_id; when no shared_key is
/// derivable, returning `Ok(SyncResult { sent: 0 })` is misleading and
/// risks the caller silently treating the contact as "synced". Returning
/// a typed `Err` is the honest signal. Pre-fix this path also produced
/// the ADR-029 stable-token fallback.
// @internal
#[test]
fn sync_contact_errors_when_load_contact_returns_none_adr029() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SyncController::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    let ratchet = create_test_ratchet();
    controller.register_ratchet("contact-adr029-b", ratchet);

    let result = controller.sync_contact("contact-adr029-b");
    assert!(
        result.is_err(),
        "sync_contact must Err when no shared_key is derivable; got {:?}",
        result
    );
}
