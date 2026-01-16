//! Protocol Layer
//!
//! Message serialization, framing, and utilities.

use super::error::NetworkError;
use super::message::{MessageEnvelope, MessagePayload, PROTOCOL_VERSION};

/// Maximum message size (1 MB).
pub const MAX_MESSAGE_SIZE: usize = 1_048_576;

/// Frame header size (4 bytes length prefix).
pub const FRAME_HEADER_SIZE: usize = 4;

/// Serializes a message envelope to bytes with length framing.
///
/// Format: [length: 4 bytes big-endian] [json payload]
pub fn encode_message(message: &MessageEnvelope) -> Result<Vec<u8>, NetworkError> {
    let json =
        serde_json::to_vec(message).map_err(|e| NetworkError::Serialization(e.to_string()))?;

    if json.len() > MAX_MESSAGE_SIZE {
        return Err(NetworkError::InvalidMessage(format!(
            "Message too large: {} bytes (max {})",
            json.len(),
            MAX_MESSAGE_SIZE
        )));
    }

    let len = json.len() as u32;
    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);

    Ok(frame)
}

/// Deserializes a message envelope from bytes (after reading frame).
///
/// Expects just the JSON payload without the length prefix.
pub fn decode_message(data: &[u8]) -> Result<MessageEnvelope, NetworkError> {
    if data.len() > MAX_MESSAGE_SIZE {
        return Err(NetworkError::InvalidMessage(format!(
            "Message too large: {} bytes (max {})",
            data.len(),
            MAX_MESSAGE_SIZE
        )));
    }

    let envelope: MessageEnvelope =
        serde_json::from_slice(data).map_err(|e| NetworkError::InvalidMessage(e.to_string()))?;

    // Version check
    if envelope.version != PROTOCOL_VERSION {
        return Err(NetworkError::InvalidMessage(format!(
            "Unsupported protocol version: {} (expected {})",
            envelope.version, PROTOCOL_VERSION
        )));
    }

    Ok(envelope)
}

/// Reads the length prefix from a frame header.
#[allow(dead_code)] // Exported for use by transport implementations
pub fn read_frame_length(header: &[u8; FRAME_HEADER_SIZE]) -> usize {
    u32::from_be_bytes(*header) as usize
}

/// Creates a new message envelope with a fresh ID and timestamp.
pub fn create_envelope(payload: MessagePayload) -> MessageEnvelope {
    MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: uuid::Uuid::new_v4().to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::message::{PresenceStatus, PresenceUpdate};

    fn create_test_envelope() -> MessageEnvelope {
        MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test-123".to_string(),
            timestamp: 1234567890,
            payload: MessagePayload::Presence(PresenceUpdate {
                status: PresenceStatus::Online,
                message: None,
            }),
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let envelope = create_test_envelope();

        let encoded = encode_message(&envelope).unwrap();

        // Skip the 4-byte length prefix
        let decoded = decode_message(&encoded[FRAME_HEADER_SIZE..]).unwrap();

        assert_eq!(decoded.version, envelope.version);
        assert_eq!(decoded.message_id, envelope.message_id);
        assert_eq!(decoded.timestamp, envelope.timestamp);
    }

    #[test]
    fn test_encode_message_with_length_prefix() {
        let envelope = create_test_envelope();
        let encoded = encode_message(&envelope).unwrap();

        // First 4 bytes should be length prefix
        let length = read_frame_length(&encoded[..4].try_into().unwrap());

        // Remaining bytes should be the JSON payload
        assert_eq!(length, encoded.len() - FRAME_HEADER_SIZE);
    }

    #[test]
    fn test_decode_rejects_oversized_message() {
        let oversized = vec![0u8; MAX_MESSAGE_SIZE + 1];
        let result = decode_message(&oversized);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    #[test]
    fn test_decode_rejects_wrong_version() {
        let mut envelope = create_test_envelope();
        envelope.version = 255; // Wrong version

        let json = serde_json::to_vec(&envelope).unwrap();
        let result = decode_message(&json);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unsupported protocol version"));
    }

    #[test]
    fn test_create_envelope_generates_unique_ids() {
        let payload = MessagePayload::Presence(PresenceUpdate {
            status: PresenceStatus::Online,
            message: None,
        });

        let env1 = create_envelope(payload.clone());
        let env2 = create_envelope(payload);

        assert_ne!(env1.message_id, env2.message_id);
    }

    #[test]
    fn test_read_frame_length() {
        let header: [u8; 4] = [0x00, 0x00, 0x01, 0x00]; // 256 in big-endian
        assert_eq!(read_frame_length(&header), 256);

        let header2: [u8; 4] = [0x00, 0x01, 0x00, 0x00]; // 65536 in big-endian
        assert_eq!(read_frame_length(&header2), 65536);
    }

    #[test]
    fn test_decode_rejects_invalid_json() {
        let invalid = b"not valid json";
        let result = decode_message(invalid);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            NetworkError::InvalidMessage(_)
        ));
    }
}
