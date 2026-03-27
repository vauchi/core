// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Message Classification
//!
//! Inspects framed wire bytes to determine the message type without full
//! deserialization. This lets clients route messages to the correct handler
//! without duplicating classification logic.

use super::simple_message::{FRAME_HEADER_SIZE, SimpleEnvelope, SimplePayload};

/// Classified message type.
///
/// Represents the type of a relay message determined from the wire format.
/// Used by clients to route incoming messages to the appropriate handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Encrypted card update (Double Ratchet encrypted payload).
    EncryptedUpdate,
    /// Delivery acknowledgment from the relay.
    Acknowledgment,
    /// Connection handshake message.
    Handshake,
    /// Identity revocation signal.
    IdentityRevoked,
    /// Signed field validation record.
    ValidationRecord,
    /// Field validation revocation.
    ValidationRevocation,
    /// Unrecognized or unparsable message.
    Unknown,
}

/// Classifies a framed wire message by inspecting its payload type tag.
///
/// Accepts the raw bytes as received from the transport (with the 4-byte
/// length prefix). Returns [`MessageType::Unknown`] for any input that
/// cannot be parsed, including empty input, truncated frames, or invalid JSON.
///
/// # Examples
///
/// ```ignore
/// use vauchi_core::network::{classify_message, MessageType};
///
/// let msg_type = classify_message(&wire_bytes);
/// match msg_type {
///     MessageType::EncryptedUpdate => { /* handle update */ }
///     MessageType::Acknowledgment => { /* handle ack */ }
///     _ => { /* handle other types */ }
/// }
/// ```
pub fn classify_message(data: &[u8]) -> MessageType {
    // Need at least the frame header
    if data.len() < FRAME_HEADER_SIZE {
        return MessageType::Unknown;
    }

    let json = &data[FRAME_HEADER_SIZE..];

    // Attempt to parse the envelope
    let envelope: SimpleEnvelope = match serde_json::from_slice(json) {
        Ok(e) => e,
        Err(_) => return MessageType::Unknown,
    };

    match envelope.payload {
        SimplePayload::EncryptedUpdate(_) => MessageType::EncryptedUpdate,
        SimplePayload::Acknowledgment(_) => MessageType::Acknowledgment,
        SimplePayload::Handshake(_) => MessageType::Handshake,
        SimplePayload::IdentityRevoked(_) => MessageType::IdentityRevoked,
        SimplePayload::ValidationRecord(_) => MessageType::ValidationRecord,
        SimplePayload::ValidationRevocation(_) => MessageType::ValidationRevocation,
        SimplePayload::Unknown => MessageType::Unknown,
    }
}
