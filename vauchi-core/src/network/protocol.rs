// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Protocol Layer
//!
//! Message serialization, framing, and utilities.

use super::error::NetworkError;
use super::message::{MessageEnvelope, MessageId, MessagePayload, PROTOCOL_VERSION};

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

    if envelope.version != PROTOCOL_VERSION {
        return Err(NetworkError::InvalidMessage(format!(
            "Unsupported protocol version: {}",
            envelope.version
        )));
    }

    Ok(envelope)
}

/// Creates a new message envelope with the given ID, timestamp, and payload.
///
/// The `message_id` must be unique per envelope. Callers typically generate it
/// via `rng.uuid_v4()` to avoid ambient non-determinism (C13).
pub fn create_envelope(
    payload: MessagePayload,
    now: u64,
    message_id: MessageId,
) -> MessageEnvelope {
    MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id,
        timestamp: now,
        payload,
    }
}

// INLINE_TEST_REQUIRED: Tests frame encoding/decoding for wire protocol parsing
#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::message::{PresenceStatus, PresenceUpdate};

    fn create_test_envelope() -> MessageEnvelope {
        MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test-123".to_string().into(),
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

        let decoded = decode_message(&encoded[FRAME_HEADER_SIZE..]).unwrap();

        assert_eq!(decoded.version, envelope.version);
        assert_eq!(decoded.message_id, envelope.message_id);
        assert_eq!(decoded.timestamp, envelope.timestamp);
    }

    #[test]
    fn test_encode_message_with_length_prefix() {
        let envelope = create_test_envelope();
        let encoded = encode_message(&envelope).unwrap();

        let length = u32::from_be_bytes(encoded[..FRAME_HEADER_SIZE].try_into().unwrap()) as usize;

        assert_eq!(length, encoded.len() - FRAME_HEADER_SIZE);
    }

    #[test]
    fn test_decode_rejects_oversized_message() {
        let oversized = vec![0u8; MAX_MESSAGE_SIZE + 1];
        let result = decode_message(&oversized);

        assert!(result.is_err(), "expected error");
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    #[test]
    fn test_decode_rejects_wrong_version() {
        let mut envelope = create_test_envelope();
        envelope.version = 255; // Wrong version

        let json = serde_json::to_vec(&envelope).unwrap();
        let result = decode_message(&json);

        assert!(result.is_err(), "expected error");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unsupported protocol version")
        );
    }

    #[test]
    fn test_create_envelope_generates_unique_ids() {
        let payload = MessagePayload::Presence(PresenceUpdate {
            status: PresenceStatus::Online,
            message: None,
        });

        let env1 = create_envelope(payload.clone(), 0, "test-msg-1".into());
        let env2 = create_envelope(payload, 0, "test-msg-2".into());

        assert_ne!(env1.message_id, env2.message_id);
    }

    #[test]
    fn test_decode_rejects_invalid_json() {
        let invalid = b"not valid json";
        let result = decode_message(invalid);

        assert!(result.is_err(), "expected error");
        assert!(matches!(
            result.unwrap_err(),
            NetworkError::InvalidMessage(_)
        ));
    }
}
