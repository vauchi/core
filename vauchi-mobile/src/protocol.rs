//! Wire protocol for relay communication.
//!
//! This module re-exports the simple message protocol from vauchi-core
//! for backward compatibility. New code should import directly from
//! `vauchi_core::network::simple_message`.

// Re-export types from vauchi-core's simple_message module
pub use vauchi_core::network::simple_message::{
    create_simple_ack as create_ack, create_simple_envelope as create_envelope,
    decode_simple_message as decode_message, encode_simple_message as encode_message,
    LegacyExchangeMessage as ExchangeMessage, SimpleAckStatus as AckStatus,
    SimpleEncryptedUpdate as EncryptedUpdate, SimpleHandshake as Handshake,
    SimplePayload as MessagePayload,
};

// Re-export for tests
#[cfg(test)]
pub use vauchi_core::network::simple_message::SIMPLE_PROTOCOL_VERSION as PROTOCOL_VERSION;

// INLINE_TEST_REQUIRED: Tests re-export compatibility layer - small module kept inline
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let handshake = Handshake {
            client_id: "test-client".to_string(),
        };
        let envelope = create_envelope(MessagePayload::Handshake(handshake));

        let encoded = encode_message(&envelope).unwrap();
        let decoded = decode_message(&encoded).unwrap();

        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.message_id, envelope.message_id);

        match decoded.payload {
            MessagePayload::Handshake(h) => assert_eq!(h.client_id, "test-client"),
            _ => panic!("Wrong payload type"),
        }
    }

    #[test]
    fn test_exchange_message_detection() {
        let exchange = ExchangeMessage {
            msg_type: "exchange".to_string(),
            identity_public_key: "abc".to_string(),
            ephemeral_public_key: "def".to_string(),
            display_name: "Alice".to_string(),
            is_response: false,
        };
        let data = serde_json::to_vec(&exchange).unwrap();

        assert!(ExchangeMessage::is_exchange(&data));
        assert!(!ExchangeMessage::is_exchange(b"not json"));
        assert!(!ExchangeMessage::is_exchange(b"{\"msg_type\":\"other\"}"));
    }
}
