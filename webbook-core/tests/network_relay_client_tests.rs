//! Tests for network::relay_client
//! Extracted from relay_client.rs

use std::time::Duration;
use webbook_core::crypto::{DoubleRatchetState, SymmetricKey};
use webbook_core::exchange::X3DHKeyPair;
use webbook_core::network::mock::MockTransport;
use webbook_core::network::*;
use webbook_core::*;

fn create_test_config() -> RelayClientConfig {
    RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 10,
        ack_timeout_ms: 100, // Short timeout for testing
        max_retries: 3,
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

    // Process ack
    let acked = client.process_incoming().unwrap();

    assert_eq!(acked.len(), 1);
    assert_eq!(acked[0], "update-1");
    assert_eq!(client.in_flight_count(), 0);
}

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

    // Wait for timeout
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Check timeouts
    let timed_out = client.check_timeouts();

    assert_eq!(timed_out.len(), 1);
    assert_eq!(timed_out[0], "update-1");
    assert_eq!(client.in_flight_count(), 0);
}

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
