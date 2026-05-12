// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Relay Simulation Tests
//!
//! Tests for relay network functionality including message delivery,
//! connection management, and error scenarios.
//! Covers the relay_network.feature scenarios.

use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, Storage, SymmetricKey, Vauchi,
    crypto::ratchet::DoubleRatchetState,
    exchange::X3DHKeyPair,
    network::{MockTransport, RelayClient, RelayClientConfig, TransportConfig},
    sync::SyncManager,
};

// =============================================================================
// Connection Management Tests
// =============================================================================

/// Test: Relay client connects and disconnects cleanly
// @internal
#[test]
fn test_relay_connect_disconnect() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
        ..Default::default()
    };

    let mut client = RelayClient::new(transport, config, "test-id".into());

    // Initially not connected
    assert!(!client.is_connected());

    // Connect
    client.connect().unwrap();
    assert!(client.is_connected());

    // Disconnect
    client.disconnect().unwrap();
    assert!(!client.is_connected());
}

/// Test: Reconnection after disconnect
// @internal
#[test]
fn test_relay_reconnection() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
        ..Default::default()
    };

    let mut client = RelayClient::new(transport, config, "test-id".into());

    // First connection cycle
    client.connect().unwrap();
    assert!(client.is_connected());
    client.disconnect().unwrap();
    assert!(!client.is_connected());

    // Reconnect
    client.connect().unwrap();
    assert!(client.is_connected());
}

// =============================================================================
// Message Delivery Tests
// =============================================================================

/// Test: Sending update through relay
// @scenario: relay_network :: Automatic fallback to relay
// @scenario: sync_updates :: Sync via WebSocket relay
// @internal
#[test]
fn test_relay_send_update() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
        ..Default::default()
    };

    let mut client = RelayClient::new(transport, config, "sender-id".into());
    client.connect().unwrap();

    // Set up encryption
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    // Send update
    let msg_id = client
        .send_update(
            0,
            "recipient-id",
            &mut ratchet,
            b"card update data",
            "update-001",
            None,
        )
        .unwrap();

    assert!(!msg_id.is_empty());
    assert_eq!(client.in_flight_count(), 1);
}

/// Test: Multiple messages tracked in-flight
// @scenario: relay_network :: Relay stores messages for offline contacts
// @internal
#[test]
fn test_relay_multiple_in_flight() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
        ..Default::default()
    };

    let mut client = RelayClient::new(transport, config, "sender-id".into());
    client.connect().unwrap();

    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    // Send multiple updates
    client
        .send_update(0, "recipient1", &mut ratchet, b"data1", "update-001", None)
        .unwrap();
    client
        .send_update(0, "recipient2", &mut ratchet, b"data2", "update-002", None)
        .unwrap();
    client
        .send_update(0, "recipient3", &mut ratchet, b"data3", "update-003", None)
        .unwrap();

    assert_eq!(client.in_flight_count(), 3);

    let update_ids = client.in_flight_update_ids();
    assert!(update_ids.contains(&"update-001".to_string()));
    assert!(update_ids.contains(&"update-002".to_string()));
    assert!(update_ids.contains(&"update-003".to_string()));
}

// =============================================================================
// Sync Manager Integration Tests
// =============================================================================

/// Test: Sync manager queues updates for delivery
// @scenario: relay_network :: Relay stores messages for offline contacts
// @internal
#[test]
fn test_sync_manager_queue_for_relay() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Alice");
    old_card
        .add_field(ContactField::new(
            FieldType::Email,
            "work",
            "old@test.com",
            0,
        ))
        .unwrap();

    let mut new_card = ContactCard::new("Alice");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "work",
            "new@test.com",
            0,
        ))
        .unwrap();

    // Queue for multiple contacts
    let update1 = sync_manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();
    let update2 = sync_manager
        .queue_card_update("contact-2", &old_card, &new_card)
        .unwrap();

    assert!(!update1.is_empty());
    assert!(!update2.is_empty());

    // Check pending for each contact
    let pending1 = sync_manager.get_pending("contact-1").unwrap();
    let pending2 = sync_manager.get_pending("contact-2").unwrap();

    assert_eq!(pending1.len(), 1);
    assert_eq!(pending2.len(), 1);
}

/// Test: Marking updates as delivered clears pending
// @internal
#[test]
fn test_sync_manager_mark_delivered() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);

    let old_card = ContactCard::new("Test");
    let new_card = ContactCard::new("Test Updated");

    let update_id = sync_manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();

    // Initially pending
    let pending = sync_manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 1);

    // Mark delivered
    sync_manager.mark_delivered(&update_id).unwrap();

    // No longer pending
    let pending = sync_manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 0);

    // State should be synced
    let state = sync_manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, vauchi_core::SyncState::Synced { .. }));
}

/// Test: Sync state reflects pending count
// @internal
#[test]
fn test_sync_state_pending_count() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);

    let card1 = ContactCard::new("V1");
    let card2 = ContactCard::new("V2");
    let card3 = ContactCard::new("V3");

    // Queue 3 updates for same contact
    sync_manager
        .queue_card_update("contact-1", &card1, &card2)
        .unwrap();
    sync_manager
        .queue_card_update("contact-1", &card2, &card3)
        .unwrap();

    let state = sync_manager.get_sync_state("contact-1").unwrap();
    if let vauchi_core::SyncState::Pending { queued_count, .. } = state {
        assert_eq!(queued_count, 2);
    } else {
        panic!("Expected Pending state");
    }
}

// =============================================================================
// Full Workflow Tests
// =============================================================================

/// Test: Complete update propagation flow
// @scenario: relay_network :: Automatic fallback to relay
// @scenario: relay_network :: Relay only sees encrypted blobs
// @scenario: sync_updates :: Sync via WebSocket relay
// @scenario: sync_updates :: All sync traffic is encrypted
// @internal
#[test]
fn test_full_update_propagation() {
    // Set up Alice and Bob
    let mut alice_wb: Vauchi = Vauchi::in_memory().unwrap();
    let mut bob_wb: Vauchi = Vauchi::in_memory().unwrap();

    alice_wb.create_identity("Alice").unwrap();
    bob_wb.create_identity("Bob").unwrap();

    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let shared_secret = SymmetricKey::generate();

    // Exchange contacts
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone());
    let bob_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_contact =
        Contact::from_exchange(alice_pk, ContactCard::new("Alice"), shared_secret.clone());
    let alice_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    // Set up ratchets
    let bob_dh = X3DHKeyPair::generate();
    let alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    alice_wb
        .storage()
        .save_ratchet_state(&bob_id, &alice_ratchet, true)
        .unwrap();
    bob_wb
        .storage()
        .save_ratchet_state(&alice_id, &bob_ratchet, false)
        .unwrap();

    // Alice updates her card
    let old_card = alice_wb.own_card().unwrap().unwrap();
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
            0,
        ))
        .unwrap();
    let new_card = alice_wb.own_card().unwrap().unwrap();

    // Propagate update
    let queued = alice_wb
        .propagate_card_update(&old_card, &new_card)
        .unwrap();
    assert_eq!(queued, 1);

    // Verify pending update exists
    let pending = alice_wb.storage().get_pending_updates(&bob_id).unwrap();
    assert_eq!(pending.len(), 1);

    // Bob decrypts and applies
    let (mut bob_ratchet, _) = bob_wb
        .storage()
        .load_ratchet_state(&alice_id)
        .unwrap()
        .unwrap();
    let ratchet_msg: vauchi_core::crypto::ratchet::RatchetMessage =
        serde_json::from_slice(&pending[0].payload).unwrap();
    let payload_bytes = bob_ratchet.decrypt(&ratchet_msg).unwrap();

    // Unwrap CEK-wrapped payload (version 0x02)
    let vp = vauchi_core::sync::delta::VersionedPayload::decode(&payload_bytes).unwrap();
    let delta_bytes = match vp {
        vauchi_core::sync::delta::VersionedPayload::CekWrapped(wrapped) => {
            let cek = vauchi_core::crypto::cek::ContentEncryptionKey::from_bytes(wrapped.cek);
            cek.decrypt(&wrapped.cek_ciphertext).unwrap()
        }
        _ => panic!("expected CekWrapped variant"),
    };
    let delta: vauchi_core::sync::CardDelta = serde_json::from_slice(&delta_bytes).unwrap();

    // Apply to Bob's view of Alice
    let bob_alice_contact = bob_wb.get_contact(&alice_id).unwrap().unwrap();
    let mut alice_card_at_bob = bob_alice_contact.card().clone();
    delta.apply(&mut alice_card_at_bob, 0).unwrap();

    // Verify Bob has the new field
    assert_eq!(alice_card_at_bob.fields().len(), 1);
    assert!(
        alice_card_at_bob
            .fields()
            .iter()
            .any(|f| f.label() == "work")
    );
}

// =============================================================================
// Configuration Tests
// =============================================================================

/// Test: Relay client config values are respected
// @scenario: relay_network :: Relay node configuration
// @internal
#[test]
fn test_relay_config() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig {
            connect_timeout_ms: 5000,
            io_timeout_ms: 10000,
            ..Default::default()
        },
        max_pending_messages: 50,
        ack_timeout_ms: 15_000,
        max_retries: 5,
        ..Default::default()
    };

    let client = RelayClient::new(transport, config, "test-id".into());
    // Client is created with config - verify it doesn't panic
    assert!(!client.is_connected());
}

/// Test: Default transport config
// @scenario: relay_network :: Relay node configuration
// @internal
#[test]
fn test_default_transport_config() {
    let config = TransportConfig::default();

    assert!(config.connect_timeout_ms > 0);
    assert!(config.io_timeout_ms > 0);
}

// =============================================================================
// Edge Cases
// =============================================================================

/// Test: Empty payload handling
// @internal
#[test]
fn test_relay_empty_payload() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
        ..Default::default()
    };

    let mut client = RelayClient::new(transport, config, "sender-id".into());
    client.connect().unwrap();

    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    // Empty payload should still work
    let result = client.send_update(0, "recipient-id", &mut ratchet, b"", "update-empty", None);
    result.expect("expected success");
}

/// Test: Large payload handling
// @scenario: relay_network :: Storage limits per blob
// @internal
#[test]
fn test_relay_large_payload() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
        ..Default::default()
    };

    let mut client = RelayClient::new(transport, config, "sender-id".into());
    client.connect().unwrap();

    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    // Large payload (100KB)
    let large_payload = vec![0xABu8; 100 * 1024];
    let result = client.send_update(
        0,
        "recipient-id",
        &mut ratchet,
        &large_payload,
        "update-large",
        None,
    );
    result.expect("expected success");
}
