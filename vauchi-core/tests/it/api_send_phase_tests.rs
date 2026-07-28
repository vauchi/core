// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `api::send_phase` (the send-phase worker of `Vauchi::sync()`,
//! formerly `SyncController` — retired by consolidation Step 3)

use std::sync::Arc;
use vauchi_core::api::*;
use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::RegistryBroadcast;
use vauchi_core::network::anonymous::{
    compute_anonymous_id, compute_anonymous_id_for_device, current_epoch,
};
use vauchi_core::network::{MessagePayload, MockTransport, RelayClientConfig, TransportConfig};
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

// @internal
#[test]
fn test_send_phase_connect_disconnect() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SendPhase::new(relay, &storage, config, events);

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
fn test_send_phase_sync_not_connected() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SendPhase::new(relay, &storage, config, events);

    let result = controller.sync(&vauchi_core::rng::OsSecureRng::new());
    assert!(matches!(result, Err(VauchiError::Network(_))));
}

// @internal
#[test]
fn test_send_phase_sync_empty() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SendPhase::new(relay, &storage, config, events);
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
fn test_send_phase_get_sync_state() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SendPhase::new(relay, &storage, config, events);

    // No pending updates = synced
    let state = controller.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, SyncState::Synced { .. }));
}

// @internal
#[test]
fn test_send_phase_pending_count() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SendPhase::new(relay, &storage, config, events);

    assert_eq!(controller.pending_count().unwrap(), 0);
}

// @internal
#[test]
fn test_send_phase_in_flight_count() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SendPhase::new(relay, &storage, config, events);

    assert_eq!(controller.in_flight_count(), 0);
}

// @internal
#[test]
fn test_send_phase_auto_sync_config() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());

    let config = SyncConfig {
        auto_sync: true,
        ..Default::default()
    };
    let controller = SendPhase::new(relay, &storage, config, events.clone());
    assert!(controller.is_auto_sync_enabled());

    let relay2 = create_test_relay();
    let config2 = SyncConfig {
        auto_sync: false,
        ..Default::default()
    };
    let controller2 = SendPhase::new(relay2, &storage, config2, events);
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
fn test_send_phase_batch_size_config() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());

    let config = SyncConfig {
        batch_size: Some(5),
        ..Default::default()
    };

    let mut controller = SendPhase::new(relay, &storage, config, events);
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

    let mut controller = SendPhase::new(relay, &storage, config, events);

    // Not connected — should fail
    let result = controller.sync_contact("contact-1");
    assert!(matches!(result, Err(VauchiError::Network(_))));
}

// @scenario: sync_updates :: Connection state tracking
// @internal
#[test]
fn test_send_phase_connection_state() {
    use vauchi_core::network::ConnectionState;

    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SendPhase::new(relay, &storage, config, events);

    assert_eq!(controller.connection_state(), ConnectionState::Disconnected);

    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert_eq!(controller.connection_state(), ConnectionState::Connected);

    controller
        .disconnect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert_eq!(controller.connection_state(), ConnectionState::Disconnected);
}

// @scenario: sync_updates :: Sync status across contacts
// @internal
#[test]
fn test_send_phase_sync_status() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SendPhase::new(relay, &storage, config, events);

    let status = controller.sync_status().unwrap();
    assert!(status.is_empty());
}

// @scenario: sync_updates :: Relay accessor methods
// @internal
#[test]
fn test_send_phase_relay_accessors() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SendPhase::new(relay, &storage, config, events);

    let _relay = controller.relay();
    assert!(!controller.is_connected());

    let _relay_mut = controller.relay_mut();

    let _sm = controller.sync_manager();

    let _rs = controller.retry_scheduler();

    let _om = controller.offline_manager();
}

// @internal
#[test]
fn test_send_phase_sync_contact_without_pending_payload_is_noop() {
    let storage = create_test_storage();
    let contact = Contact::from_exchange(
        [0x71; 32],
        ContactCard::new("No pending"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SendPhase::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    let result = controller.sync_contact(&contact_id);
    assert_eq!(result.unwrap().total(), 0);
}

// @internal
#[test]
fn test_send_phase_sync_contact_with_ratchet() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let mut controller = SendPhase::new(relay, &storage, config, events);
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
    storage.contacts().save_contact(&contact).unwrap();

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
fn test_send_phase_process_device_sync() {
    let storage = create_test_storage();
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();

    let controller = SendPhase::new(relay, &storage, config, events);

    let master_seed = [0x42u8; 32];
    let device = create_test_device(&master_seed, 0, "Test Device");
    let registry = create_test_registry(&master_seed, &device);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device, registry);

    let incoming = vec![SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+1234567890".to_string(),
        timestamp: 1000,
    }];

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

    let mut controller = SendPhase::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    // Contact NOT saved to storage — load_contact will return Ok(None).
    let update = PendingUpdate {
        id: "u-adr029-1".to_string(),
        contact_id: "contact-adr029".to_string(),
        update_type: "card_update".to_string(),
        payload: vec![1, 2, 3],
        created_at: 0,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
        target_device_id: None,
    };
    storage
        .pending()
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

    let mut controller = SendPhase::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    let result = controller.sync_contact("contact-adr029-b");
    assert!(
        result.is_err(),
        "sync_contact must Err when no shared_key is derivable; got {:?}",
        result
    );
}

// Regression (2026-06-30): the send loop counted `sent` but never cleared the
// pending update, so it re-sent the SAME ratchet message every sync — which the
// receiver decrypt-fails (its ratchet advanced past that message), churning the
// receive path and burying real card updates. The intended ack-based clear never
// fired because the `RelayClient` is rebuilt each sync, losing the in-flight map.
// A successful send (relay-accept = store-and-forward) must clear the queue.
// @scenario: sync_updates :: A successfully-sent update is cleared from the queue
#[test]
fn test_sync_send_clears_pending_update_on_relay_accept() {
    use vauchi_core::storage::{PendingUpdate, UpdateStatus};

    let storage = create_test_storage();

    // A sendable contact: shared key + public key (the directional token needs both).
    let shared = SymmetricKey::generate();
    let peer_pk = [0x33u8; 32];
    let contact = Contact::from_exchange(peer_pk, ContactCard::new("Peer"), shared.clone(), 0);
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    storage
        .device()
        .save_device_info(&[0x55; 32], 0, "Local", 0)
        .unwrap();

    // Role-correct ratchet + a queued update carrying a real ratchet message.
    let peer_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared, *peer_dh.public_key()).unwrap();
    let msg = ratchet.encrypt(b"card-delta").unwrap();
    storage
        .ratchets()
        .save_ratchet_state_for_device(&contact_id, &[0x44; 32], &ratchet, true)
        .unwrap();
    let update = PendingUpdate {
        id: "u1".to_string(),
        contact_id: contact_id.clone(),
        update_type: "card_delta".to_string(),
        payload: serde_json::to_vec(&msg).unwrap(),
        created_at: 0,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
        target_device_id: None,
    };
    storage.pending().queue_update(&update).unwrap();
    assert_eq!(
        storage
            .pending()
            .get_pending_updates(&contact_id)
            .unwrap()
            .len(),
        1,
        "precondition: one update queued"
    );

    // Drive a real send through the controller (MockTransport accepts).
    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let config = SyncConfig::default();
    let mut controller = SendPhase::new(relay, &storage, config, events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    let result = controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    assert_eq!(
        result.sent, 1,
        "a device-scoped pending update must not require a legacy controller ratchet"
    );
    let sent = controller.relay().connection().transport().sent_messages();
    let MessagePayload::EncryptedUpdate(envelope) = &sent[0].payload else {
        panic!("expected encrypted update")
    };
    assert_eq!(
        envelope.sender_id,
        hex::encode(compute_anonymous_id(
            shared.as_bytes(),
            current_epoch(storage.clock().unix_seconds())
        )),
        "a peer without an ADR-0064 registry can resolve only the legacy token"
    );
    assert_eq!(
        storage
            .pending()
            .get_pending_updates(&contact_id)
            .unwrap()
            .len(),
        0,
        "a successfully-sent update must be cleared so it is not re-sent every sync"
    );
}

// @scenario: multi_device_sync :: A device-scoped send requires local device info
#[test]
fn test_sync_send_does_not_downgrade_modern_peer_without_local_device_info() {
    use vauchi_core::storage::{PendingUpdate, UpdateStatus};

    let storage = create_test_storage();
    let peer_signing = SigningKeyPair::from_seed(&[0x61; 32]);
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *peer_signing.public_key().as_bytes(),
        ContactCard::new("Modern peer"),
        shared.clone(),
        0,
    );
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    let peer_seed = [0x62; 32];
    let peer_device = DeviceInfo::derive(&peer_seed, 0, "Peer phone".into(), 1);
    let peer_registry = DeviceRegistry::new(peer_device.to_registered(&peer_seed), &peer_signing);
    let broadcast = RegistryBroadcast::new(
        &peer_registry,
        &peer_signing,
        storage.clock().unix_seconds(),
    );
    storage
        .device()
        .save_contact_device_registry(
            &contact_id,
            &broadcast,
            peer_signing.public_key().as_bytes(),
            60,
        )
        .unwrap();

    let peer_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared, *peer_dh.public_key()).unwrap();
    let message = ratchet.encrypt(b"device-scoped update").unwrap();
    storage
        .pending()
        .queue_update(&PendingUpdate {
            id: "modern-no-local-device".into(),
            contact_id: contact_id.clone(),
            update_type: "card_delta".into(),
            payload: serde_json::to_vec(&message).unwrap(),
            created_at: 0,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
            // A device-scoped card delta (not a genesis handshake) — the
            // sender token must identify THIS device, so local device info
            // is required and its absence is a hard error.
            target_device_id: Some(*peer_device.device_id()),
        })
        .unwrap();

    let relay = create_test_relay();
    let events = Arc::new(EventDispatcher::new());
    let mut controller = SendPhase::new(relay, &storage, SyncConfig::default(), events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    let result = controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    assert_eq!(result.sent, 0);
    assert_eq!(result.failed, 1);
    assert_eq!(
        result.errors,
        vec![(
            contact_id.clone(),
            "invalid state: local device info is required for a device-scoped sender token".into()
        )]
    );
    assert_eq!(
        storage
            .pending()
            .get_pending_updates(&contact_id)
            .unwrap()
            .len(),
        1
    );
}

/// Shared setup for the sender-token tests: a contact we can send to, with
/// one peer device we already know (sibling-synced) and our own device info
/// present. Returns `(storage, contact_id, shared_key, known_peer_device_id)`.
fn storage_with_known_peer_device(
    peer_seed_byte: u8,
    our_device_id: [u8; 32],
    persist_local_device: bool,
) -> (Storage, String, SymmetricKey, [u8; 32]) {
    let storage = create_test_storage();
    let peer_signing = SigningKeyPair::from_seed(&[peer_seed_byte; 32]);
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *peer_signing.public_key().as_bytes(),
        ContactCard::new("Bootstrapping peer"),
        shared.clone(),
        0,
    );
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    let peer_seed = [peer_seed_byte.wrapping_add(1); 32];
    let peer_device = DeviceInfo::derive(&peer_seed, 0, "Peer phone".into(), 1);
    let peer_registry = DeviceRegistry::new(peer_device.to_registered(&peer_seed), &peer_signing);
    let broadcast = RegistryBroadcast::new(
        &peer_registry,
        &peer_signing,
        storage.clock().unix_seconds(),
    );
    storage
        .device()
        .save_contact_device_registry(
            &contact_id,
            &broadcast,
            peer_signing.public_key().as_bytes(),
            60,
        )
        .unwrap();
    if persist_local_device {
        storage
            .device()
            .save_device_info(&our_device_id, 0, "Local", 0)
            .unwrap();
    }

    let known_peer_device_id = storage
        .device()
        .load_contact_active_devices(&contact_id)
        .unwrap()[0]
        .device_id;
    (storage, contact_id, shared, known_peer_device_id)
}

fn queue_ratchet_update(
    storage: &Storage,
    contact_id: &str,
    shared: &SymmetricKey,
    update_type: &str,
    target_device_id: Option<[u8; 32]>,
) {
    use vauchi_core::storage::{PendingUpdate, UpdateStatus};
    let peer_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(shared, *peer_dh.public_key()).unwrap();
    let msg = ratchet.encrypt(b"payload").unwrap();
    storage
        .pending()
        .queue_update(&PendingUpdate {
            id: format!("{update_type}-update"),
            contact_id: contact_id.to_string(),
            update_type: update_type.to_string(),
            payload: serde_json::to_vec(&msg).unwrap(),
            created_at: 0,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
            target_device_id,
        })
        .unwrap();
}

fn drive_send_sender_id(storage: &Storage) -> String {
    let events = Arc::new(EventDispatcher::new());
    let mut controller =
        SendPhase::new(create_test_relay(), storage, SyncConfig::default(), events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    let result = controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert_eq!(result.sent, 1, "the update must be sent");
    let sent = controller.relay().connection().transport().sent_messages();
    let MessagePayload::EncryptedUpdate(envelope) = &sent[0].payload else {
        panic!("expected encrypted update")
    };
    envelope.sender_id.to_string()
}

fn drive_send_sender_id_with_local_device(storage: &Storage, local_device_id: [u8; 32]) -> String {
    let events = Arc::new(EventDispatcher::new());
    let mut controller =
        SendPhase::new(create_test_relay(), storage, SyncConfig::default(), events)
            .with_local_device_id(local_device_id);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    let result = controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    assert_eq!(result.sent, 1, "the update must be sent");
    let sent = controller.relay().connection().transport().sent_messages();
    let MessagePayload::EncryptedUpdate(envelope) = &sent[0].payload else {
        panic!("expected encrypted update")
    };
    envelope.sender_id.to_string()
}

fn legacy_token(storage: &Storage, shared: &SymmetricKey) -> String {
    hex::encode(compute_anonymous_id(
        shared.as_bytes(),
        current_epoch(storage.clock().unix_seconds()),
    ))
}

// A genesis handshake push rides the IDENTITY mailbox (`target_device_id:
// None`) precisely because the peer may not know this device yet. Even when we
// already know one of the peer's devices, the envelope sender id MUST be the
// legacy token, or the unacquainted peer cannot resolve the contact and drops
// the bootstrap push — the F4 lost-primary root cause (2026-07-26).
// @scenario: multi_device_sync :: An identity-mailbox send uses the legacy sender token
#[test]
fn test_identity_mailbox_send_uses_legacy_token_even_when_peer_device_known() {
    let (storage, contact_id, shared, _) = storage_with_known_peer_device(0x71, [0x77; 32], true);
    queue_ratchet_update(&storage, &contact_id, &shared, "registry_handshake", None);
    assert_eq!(
        drive_send_sender_id(&storage),
        legacy_token(&storage, &shared),
        "an identity-mailbox handshake push must use the legacy sender token so a peer that \
         does not yet know this device can resolve the contact"
    );
}

// The handshake ACK routes device-scoped (`target_device_id: Some`) so a
// sibling cannot drain it, but the peer that pushed to us may not know this
// device yet — so the envelope sender id must STILL be the legacy token.
// @scenario: multi_device_sync :: A handshake ack routes device-scoped but signs with the legacy token
#[test]
fn test_handshake_ack_uses_legacy_sender_token_despite_device_routing() {
    let (storage, contact_id, shared, peer_device_id) =
        storage_with_known_peer_device(0x81, [0x88; 32], true);
    queue_ratchet_update(
        &storage,
        &contact_id,
        &shared,
        "registry_handshake",
        Some(peer_device_id),
    );
    assert_eq!(
        drive_send_sender_id(&storage),
        legacy_token(&storage, &shared),
        "a genesis handshake ack must use the legacy sender token even though it is routed \
         to a specific peer device's mailbox"
    );
}

// A post-`Active` card delta targets a specific known peer device and the peer
// knows us, so it correctly uses this device's scoped sender token.
// @scenario: multi_device_sync :: A device-scoped card delta uses this device's sender token
#[test]
fn test_device_scoped_card_delta_uses_device_sender_token() {
    let our_device_id = [0x99u8; 32];
    let (storage, contact_id, shared, peer_device_id) =
        storage_with_known_peer_device(0x91, our_device_id, true);
    queue_ratchet_update(
        &storage,
        &contact_id,
        &shared,
        "card_delta",
        Some(peer_device_id),
    );
    assert_eq!(
        drive_send_sender_id(&storage),
        hex::encode(compute_anonymous_id_for_device(
            shared.as_bytes(),
            current_epoch(storage.clock().unix_seconds()),
            &our_device_id,
        )),
        "a device-scoped card delta to a peer that knows us uses this device's sender token"
    );
}

// Production constructs the send phase with the authenticated Identity's
// device ID. The legacy device_info row is not populated on linked devices.
// @scenario: multi_device_sync :: A linked device sends without legacy local device storage
#[test]
fn test_device_scoped_send_uses_authenticated_local_device_id_without_storage_row() {
    let authenticated_device_id = [0xA1u8; 32];
    let (storage, contact_id, shared, peer_device_id) =
        storage_with_known_peer_device(0xA2, [0x55; 32], false);
    queue_ratchet_update(
        &storage,
        &contact_id,
        &shared,
        "card_delta",
        Some(peer_device_id),
    );

    assert_eq!(
        drive_send_sender_id_with_local_device(&storage, authenticated_device_id),
        hex::encode(compute_anonymous_id_for_device(
            shared.as_bytes(),
            current_epoch(storage.clock().unix_seconds()),
            &authenticated_device_id,
        )),
    );
}

// The authenticated Identity is authoritative over vestigial storage state:
// a stale row must not stamp an origin token the peer cannot resolve.
// @scenario: multi_device_sync :: Authenticated local device identity wins over stale storage
#[test]
fn test_authenticated_local_device_id_wins_over_stored_device_info() {
    let authenticated_device_id = [0xB1u8; 32];
    let stale_device_id = [0xB2u8; 32];
    let (storage, contact_id, shared, peer_device_id) =
        storage_with_known_peer_device(0xB3, stale_device_id, true);
    queue_ratchet_update(
        &storage,
        &contact_id,
        &shared,
        "card_delta",
        Some(peer_device_id),
    );

    assert_eq!(
        drive_send_sender_id_with_local_device(&storage, authenticated_device_id),
        hex::encode(compute_anonymous_id_for_device(
            shared.as_bytes(),
            current_epoch(storage.clock().unix_seconds()),
            &authenticated_device_id,
        )),
    );
}
