//! Tests for network::simple_message
//! Extracted from simple_message.rs

use webbook_core::network::simple_message::*;
use webbook_core::network::*;
use webbook_core::*;

#[test]
fn test_encode_decode_roundtrip() {
    let handshake = SimpleHandshake {
        client_id: "test-client".to_string(),
    };
    let envelope = create_simple_envelope(SimplePayload::Handshake(handshake));

    let encoded = encode_simple_message(&envelope).unwrap();
    let decoded = decode_simple_message(&encoded).unwrap();

    assert_eq!(decoded.version, SIMPLE_PROTOCOL_VERSION);
    assert_eq!(decoded.message_id, envelope.message_id);

    match decoded.payload {
        SimplePayload::Handshake(h) => assert_eq!(h.client_id, "test-client"),
        _ => panic!("Wrong payload type"),
    }
}

#[test]
fn test_legacy_exchange_message() {
    let msg = LegacyExchangeMessage::new("abc123", "def456", "Alice");
    assert_eq!(msg.msg_type, "exchange");
    assert!(!msg.is_response);

    let bytes = msg.to_bytes();
    assert!(LegacyExchangeMessage::is_exchange(&bytes));

    let parsed = LegacyExchangeMessage::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.display_name, "Alice");
}

#[test]
fn test_legacy_exchange_response() {
    let msg = LegacyExchangeMessage::new_response("abc123", "def456", "Bob");
    assert!(msg.is_response);
}

#[test]
fn test_simple_ack() {
    let ack = create_simple_ack("msg-123", SimpleAckStatus::Delivered);

    match ack.payload {
        SimplePayload::Acknowledgment(a) => {
            assert_eq!(a.message_id, "msg-123");
        }
        _ => panic!("Wrong payload type"),
    }
}

#[test]
fn test_encrypted_update() {
    let update = SimpleEncryptedUpdate {
        recipient_id: "recipient-abc".to_string(),
        sender_id: "sender-xyz".to_string(),
        ciphertext: vec![1, 2, 3, 4, 5],
    };

    let envelope = create_simple_envelope(SimplePayload::EncryptedUpdate(update));
    let encoded = encode_simple_message(&envelope).unwrap();
    let decoded = decode_simple_message(&encoded).unwrap();

    match decoded.payload {
        SimplePayload::EncryptedUpdate(u) => {
            assert_eq!(u.recipient_id, "recipient-abc");
            assert_eq!(u.ciphertext, vec![1, 2, 3, 4, 5]);
        }
        _ => panic!("Wrong payload type"),
    }
}
