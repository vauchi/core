// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for network::relay_client
//! Extracted from relay_client.rs

use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::network::*;

fn create_test_config() -> RelayClientConfig {
    RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 10,
        ack_timeout_ms: 100, // Short timeout for testing
        max_retries: 3,
        delivery_receipts_enabled: true,
        suppress_presence: false,
    }
}

fn create_test_ratchet() -> (DoubleRatchetState, DoubleRatchetState) {
    let _alice_dh = X3DHKeyPair::generate();
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();

    let alice = DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let bob = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    (alice, bob)
}

// @scenario: relay_network:Automatic fallback to relay
#[test]
fn test_relay_client_connect_disconnect() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "test-id".into());

    assert!(!client.is_connected());

    client.connect().unwrap();
    assert!(client.is_connected());

    client.disconnect().unwrap();
    assert!(!client.is_connected());
}

// @scenario: relay_network:Relay stores messages for offline contacts
#[test]
fn test_relay_client_send_update() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let (mut alice_ratchet, _bob_ratchet) = create_test_ratchet();
    let payload = b"Hello, Bob!";

    let msg_id = client
        .send_update("recipient-id", &mut alice_ratchet, payload, "update-1")
        .unwrap();

    assert!(!msg_id.is_empty());
    assert_eq!(client.in_flight_count(), 1);

    // Check the message was sent
    let sent = client.connection().transport().sent_messages();
    assert_eq!(sent.len(), 1);

    if let MessagePayload::EncryptedUpdate(update) = &sent[0].payload {
        assert_eq!(update.recipient_id, "recipient-id");
        assert_eq!(update.sender_id, "sender-id");
    } else {
        panic!("Expected EncryptedUpdate");
    }
}

// @scenario: message_delivery:Receive acknowledgment when update is delivered
#[test]
fn test_relay_client_acknowledgment_tracking() {
    let mut transport = MockTransport::new();
    transport.set_auto_ack(true);

    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let (mut alice_ratchet, _) = create_test_ratchet();

    // Send a message
    let _msg_id = client
        .send_update("recipient-id", &mut alice_ratchet, b"test", "update-1")
        .unwrap();

    assert_eq!(client.in_flight_count(), 1);

    // Process ack — returns IncomingResult with acknowledged IDs and ACK events
    let result = client.process_incoming().unwrap();

    assert_eq!(result.acknowledged.len(), 1);
    assert_eq!(result.acknowledged[0], "update-1");
    assert_eq!(client.in_flight_count(), 0);

    // Verify ACK events include delivery status
    assert_eq!(result.ack_events.len(), 1);
    assert_eq!(result.ack_events[0].update_id, "update-1");
    assert_eq!(result.ack_events[0].status, AckStatus::Delivered);
}

// @scenario: message_delivery:Failed ACK captured in ack_events
#[test]
fn test_process_incoming_captures_failed_ack_events() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let (mut alice_ratchet, _) = create_test_ratchet();

    // Send a message
    let msg_id = client
        .send_update("recipient-id", &mut alice_ratchet, b"test", "update-fail")
        .unwrap();

    // Manually queue a Failed ACK
    let failed_ack = MessageEnvelope {
        version: 1,
        message_id: uuid::Uuid::new_v4().to_string(),
        timestamp: 0,
        payload: MessagePayload::Acknowledgment(Acknowledgment {
            message_id: msg_id,
            status: AckStatus::Failed,
            error: Some("relay overloaded".to_string()),
        }),
    };
    client
        .connection_mut()
        .transport_mut()
        .queue_receive(failed_ack);

    let result = client.process_incoming().unwrap();

    // Failed ACKs should NOT appear in acknowledged list
    assert!(
        result.acknowledged.is_empty(),
        "Failed ACK should not be in acknowledged list"
    );

    // But they SHOULD appear in ack_events for delivery tracking
    assert_eq!(result.ack_events.len(), 1);
    assert_eq!(result.ack_events[0].update_id, "update-fail");
    assert_eq!(result.ack_events[0].status, AckStatus::Failed);
    assert_eq!(
        result.ack_events[0].error.as_deref(),
        Some("relay overloaded")
    );
}

// @scenario: message_delivery:Automatic retry on transient failure
#[test]
fn test_relay_client_timeout_detection() {
    let transport = MockTransport::new();
    let mut config = create_test_config();
    config.ack_timeout_ms = 1; // Very short timeout

    let mut client = RelayClient::new(transport, config, "sender-id".into());
    client.connect().unwrap();

    let (mut alice_ratchet, _) = create_test_ratchet();

    // Send a message
    client
        .send_update("recipient-id", &mut alice_ratchet, b"test", "update-1")
        .unwrap();

    // Poll until timeout is detected (CC-06: no bare sleeps for synchronization)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let timed_out = loop {
        let result = client.check_timeouts();
        if !result.is_empty() {
            break result;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Timed out waiting for message timeout detection"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };

    assert_eq!(timed_out.len(), 1);
    assert_eq!(timed_out[0], "update-1");
    assert_eq!(client.in_flight_count(), 0);
}

// @scenario: message_delivery:Handle quota exceeded
#[test]
fn test_relay_client_max_pending_limit() {
    let transport = MockTransport::new();
    let mut config = create_test_config();
    config.max_pending_messages = 2;

    let mut client = RelayClient::new(transport, config, "sender-id".into());
    client.connect().unwrap();

    let (mut alice_ratchet, _) = create_test_ratchet();

    // Send up to limit
    client
        .send_update("r1", &mut alice_ratchet, b"1", "u1")
        .unwrap();
    client
        .send_update("r2", &mut alice_ratchet, b"2", "u2")
        .unwrap();

    // Third should fail
    let result = client.send_update("r3", &mut alice_ratchet, b"3", "u3");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Too many pending"));
}

// @scenario: message_delivery:See delivery status for updates
#[test]
fn test_relay_client_in_flight_update_ids() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let (mut alice_ratchet, _) = create_test_ratchet();

    client
        .send_update("r1", &mut alice_ratchet, b"1", "update-a")
        .unwrap();
    client
        .send_update("r2", &mut alice_ratchet, b"2", "update-b")
        .unwrap();

    let ids = client.in_flight_update_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"update-a".to_string()));
    assert!(ids.contains(&"update-b".to_string()));
}

// @scenario: message_delivery:See delivery status for updates
#[test]
fn test_relay_client_has_in_flight() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    assert!(!client.has_in_flight());

    let (mut alice_ratchet, _) = create_test_ratchet();
    client
        .send_update("r1", &mut alice_ratchet, b"1", "u1")
        .unwrap();

    assert!(client.has_in_flight());
}

// @scenario: relay_network:Relay only sees encrypted blobs
#[test]
fn test_relay_client_send_raw_update() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let (mut alice_ratchet, _) = create_test_ratchet();

    // Encrypt externally
    let ratchet_msg = alice_ratchet.encrypt(b"raw message").unwrap();

    // Send raw
    let msg_id = client
        .send_raw_update("recipient-id", &ratchet_msg, "raw-update-1")
        .unwrap();

    assert!(!msg_id.is_empty());
    assert_eq!(client.in_flight_count(), 1);
}

// @scenario: message_delivery:See delivery status for updates
#[test]
fn test_process_result_default() {
    let result = ProcessResult::default();
    assert_eq!(result.sent, 0);
    assert_eq!(result.acknowledged, 0);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.failed, 0);
    assert!(result.message_ids.is_empty());
    assert!(result.errors.is_empty());
}

// @scenario: device_management:Changes sync between devices
#[test]
fn test_relay_client_send_device_sync_message() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let sender_device_id = [1u8; 32];
    let target_device_id = [2u8; 32];
    let ciphertext = vec![10u8; 50]; // nonce (12) + encrypted payload (38)
    let nonce = [3u8; 12];
    let sync_version = 42u64;

    let msg_id = client
        .send_device_sync_message(
            &sender_device_id,
            &target_device_id,
            ciphertext.clone(),
            nonce,
            sync_version,
        )
        .unwrap();

    assert!(!msg_id.is_empty());

    // Check the message was sent
    let sent = client.connection().transport().sent_messages();
    assert_eq!(sent.len(), 1);

    if let MessagePayload::DeviceSync(sync_msg) = &sent[0].payload {
        assert_eq!(sync_msg.sender_device_id, sender_device_id);
        assert_eq!(sync_msg.target_device_id, target_device_id);
        assert_eq!(sync_msg.ciphertext, ciphertext);
        assert_eq!(sync_msg.nonce, nonce);
        assert_eq!(sync_msg.sync_version, sync_version);
    } else {
        panic!("Expected DeviceSync message");
    }
}

// @scenario: relay_network:Automatic cleanup of stale blobs
#[test]
fn test_send_purge_request() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let request = PurgeRequest {
        public_key: [0x42; 32],
        signature: vec![0xAB; 64],
        purge_token: [0xCD; 32],
        timestamp: 1700000000,
    };

    let msg_id = client.send_purge_request(&request).unwrap();
    assert!(!msg_id.is_empty());

    let sent = client.connection().transport().sent_messages();
    assert_eq!(sent.len(), 1);

    if let MessagePayload::PurgeRequest(r) = &sent[0].payload {
        assert_eq!(r.public_key, [0x42; 32]);
        assert_eq!(r.purge_token, [0xCD; 32]);
        assert_eq!(r.timestamp, 1700000000);
    } else {
        panic!("Expected PurgeRequest message");
    }
}

// @scenario: relay_network:Automatic cleanup of stale blobs
#[test]
fn test_send_purge_request_send_error() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    // Inject error after connecting so the send fails
    client
        .connection_mut()
        .transport_mut()
        .inject_error(NetworkError::SendFailed("connection lost".into()));

    let request = PurgeRequest {
        public_key: [0x01; 32],
        signature: vec![0x02; 64],
        purge_token: [0x03; 32],
        timestamp: 1700000000,
    };

    let result = client.send_purge_request(&request);
    assert!(result.is_err());
}

// === Delivery Receipts Privacy Tests (SP-12b Phase 2) ===

// @scenario: message_delivery:Delivery receipts can be disabled by user
// @scenario: message_delivery.feature:Delivery receipts are optional
#[test]
fn test_delivery_receipts_disabled_config() {
    let config = RelayClientConfig {
        delivery_receipts_enabled: false,
        ..create_test_config()
    };
    let client = RelayClient::new(MockTransport::new(), config, "sender-id".into());
    assert!(
        !client.config().delivery_receipts_enabled,
        "Delivery receipts should be disabled"
    );
}

// @scenario: message_delivery:Delivery receipts enabled by default
#[test]
fn test_delivery_receipts_enabled_by_default() {
    let config = RelayClientConfig::default();
    assert!(
        config.delivery_receipts_enabled,
        "Delivery receipts should be enabled by default"
    );
}

// === Suppress Presence Privacy Tests (SP-12b Phase 2) ===

// @scenario: message_delivery:Suppress presence hides online status from relay
#[test]
fn test_suppress_presence_included_in_handshake() {
    use vauchi_core::identity::Identity;

    let config = RelayClientConfig {
        suppress_presence: true,
        ..create_test_config()
    };
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, config, "sender-id".into());

    let identity = Identity::create("Test User");
    client.connection_mut().set_identity(identity);
    client.connect().unwrap();

    // Verify suppress_presence is in the handshake JSON
    let sent_raw = client.connection().transport().sent_raw();
    assert!(!sent_raw.is_empty(), "Handshake should have been sent");

    let json: serde_json::Value = serde_json::from_slice(&sent_raw[0][4..]).unwrap();
    assert_eq!(
        json["payload"]["suppress_presence"], true,
        "Handshake should include suppress_presence=true from config"
    );
}

// @scenario: message_delivery:Suppress presence defaults to false
#[test]
fn test_suppress_presence_defaults_to_false() {
    let config = RelayClientConfig::default();
    assert!(
        !config.suppress_presence,
        "suppress_presence should default to false"
    );
}
