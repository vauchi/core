// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device link relay transport via HTTP exchange broker.
//!
//! Uses two exchange cycles (offer/claim/complete) to implement the
//! request/response pattern for device linking over the relay's V2 HTTP API.
//!
//! ## Flow
//!
//! **Existing device (initiator)**:
//! 1. `exchange_offer(identity_info)` → `code` (embedded in QR)
//! 2. `exchange_complete(code)` → polls until claimed → gets `{request, response_code}`
//! 3. `exchange_claim(response_code, response)` → sends response
//!
//! **New device (responder)**:
//! 1. `exchange_offer("")` → `response_code`
//! 2. `exchange_claim(code, {request, response_code})` → gets identity_info
//! 3. `exchange_complete(response_code)` → polls until claimed → gets response
//!
//! TODO(B): Migrate to dedicated `/v2/device-link` relay endpoints for
//! a single-round-trip flow. See problem record.

use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use thiserror::Error;
use vauchi_core::network::{HttpTransport, HttpTransportConfig, ProxyConfig};

/// Errors from device link relay operations.
#[derive(Error, Debug)]
pub enum DeviceLinkError {
    #[error("Failed to decode DeviceLinkRelayMessage: {0}")]
    DecodeFailed(#[from] serde_json::Error),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Timed out waiting for device link response")]
    ResponseTimeout,

    #[error("Timed out waiting for device link request")]
    RequestTimeout,

    #[error("Exchange offer failed: {0}")]
    OfferFailed(String),

    #[error("Exchange claim failed: {0}")]
    ClaimFailed(String),
}

/// Payload sent by the new device in the claim step.
/// Contains the encrypted request and a response_code for the return channel.
#[derive(serde::Serialize, serde::Deserialize)]
struct ClaimPayload {
    request: Vec<u8>,
    response_code: String,
}

fn create_transport(relay_url: &str) -> HttpTransport {
    HttpTransport::new(HttpTransportConfig {
        relay_url: relay_url.to_string(),
        timeout_ms: 10_000,
        proxy: ProxyConfig::None,
        allow_direct: true,
        pinned_certs: vec![],
    })
}

/// A device link message (kept for serialization compat with callers).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceLinkRelayMessage {
    pub target_identity: String,
    pub sender_token: String,
    pub payload: Vec<u8>,
}

/// Serialize a `DeviceLinkRelayMessage` to JSON bytes.
#[cfg(test)]
fn encode_device_link_message(msg: &DeviceLinkRelayMessage) -> Vec<u8> {
    serde_json::to_vec(msg).expect("DeviceLinkRelayMessage serialization should not fail")
}

/// Deserialize a `DeviceLinkRelayMessage` from JSON bytes.
#[cfg(test)]
fn decode_device_link_message(data: &[u8]) -> Result<DeviceLinkRelayMessage, DeviceLinkError> {
    Ok(serde_json::from_slice(data)?)
}

/// Send a device link request via relay and wait for the response.
///
/// Used by the **new device** (responder). Two exchange cycles:
/// 1. Create a return channel via `exchange_offer`
/// 2. Claim the existing device's offer with our request + return code
/// 3. Poll the return channel for the existing device's response
pub fn send_and_receive(
    relay_url: &str,
    message: &DeviceLinkRelayMessage,
    timeout_secs: u64,
) -> Result<Vec<u8>, DeviceLinkError> {
    let transport = create_transport(relay_url);

    // 1. Create return channel
    let response_code = transport
        .exchange_offer(&BASE64.encode(b""), Some(timeout_secs))
        .map_err(|e| DeviceLinkError::OfferFailed(e.to_string()))?;

    // 2. Claim the existing device's offer (code = sender_token from QR)
    let claim_payload = ClaimPayload {
        request: message.payload.clone(),
        response_code: response_code.clone(),
    };
    let claim_json = serde_json::to_vec(&claim_payload)
        .map_err(|e| DeviceLinkError::ClaimFailed(e.to_string()))?;

    let _identity_info = transport
        .exchange_claim(&message.sender_token, &BASE64.encode(&claim_json))
        .map_err(|e| DeviceLinkError::ClaimFailed(e.to_string()))?;

    // 3. Poll for response on the return channel
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(DeviceLinkError::ResponseTimeout);
        }
        match transport.exchange_complete(&response_code) {
            Ok(Some(response_b64)) => {
                let bytes = BASE64
                    .decode(&response_b64)
                    .map_err(|e| DeviceLinkError::Network(format!("decode: {e}")))?;
                return Ok(bytes);
            }
            Ok(None) => {
                thread::sleep(Duration::from_secs(1));
            }
            Err(e) => return Err(DeviceLinkError::Network(e.to_string())),
        }
    }
}

/// Listen for an incoming device link request via relay.
///
/// Used by the **existing device** (initiator):
/// 1. Post an offer with our identity info → get code (for QR)
/// 2. Poll `exchange_complete` until the new device claims it
/// 3. Return the request payload and response_code (as sender_token)
///
/// Returns `(code, payload, sender_token)` where `code` is the exchange
/// code to embed in the QR, and `sender_token` is the response_code.
pub fn create_offer_and_listen(
    relay_url: &str,
    identity_id: &str,
    timeout_secs: u64,
) -> Result<(String, Vec<u8>, String), DeviceLinkError> {
    let transport = create_transport(relay_url);

    // 1. Create offer with identity info
    let code = transport
        .exchange_offer(&BASE64.encode(identity_id.as_bytes()), Some(timeout_secs))
        .map_err(|e| DeviceLinkError::OfferFailed(e.to_string()))?;

    // 2. Poll until claimed
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(DeviceLinkError::RequestTimeout);
        }
        match transport.exchange_complete(&code) {
            Ok(Some(claim_b64)) => {
                let claim_bytes = BASE64
                    .decode(&claim_b64)
                    .map_err(|e| DeviceLinkError::Network(format!("decode: {e}")))?;
                let claim: ClaimPayload =
                    serde_json::from_slice(&claim_bytes).map_err(DeviceLinkError::DecodeFailed)?;
                return Ok((code, claim.request, claim.response_code));
            }
            Ok(None) => {
                thread::sleep(Duration::from_secs(1));
            }
            Err(e) => return Err(DeviceLinkError::Network(e.to_string())),
        }
    }
}

/// Listen for an incoming device link request (legacy API adapter).
///
/// Wraps `create_offer_and_listen` — the caller already has the code from QR
/// generation, so this just polls for the claim. But since the exchange broker
/// requires the code to be generated by `exchange_offer`, we generate a new
/// one here and return the payload + sender_token.
pub fn listen_for_request(
    relay_url: &str,
    identity_id: &str,
    timeout_secs: u64,
) -> Result<(Vec<u8>, String), DeviceLinkError> {
    let (_code, payload, sender_token) =
        create_offer_and_listen(relay_url, identity_id, timeout_secs)?;
    Ok((payload, sender_token))
}

/// Send a device link response back via relay.
///
/// Used by the **existing device** (initiator) to claim the return channel
/// created by the new device, depositing the encrypted response.
pub fn send_response(
    relay_url: &str,
    sender_token: &str,
    response_payload: Vec<u8>,
) -> Result<(), DeviceLinkError> {
    let transport = create_transport(relay_url);

    transport
        .exchange_claim(sender_token, &BASE64.encode(&response_payload))
        .map_err(|e| DeviceLinkError::ClaimFailed(e.to_string()))?;

    Ok(())
}

// INLINE_TEST_REQUIRED: Tests verify internal encode/decode and ClaimPayload serialization
#[cfg(test)]
mod tests {
    use super::*;

    // @scenario: device_sync:Device link message encoding roundtrip
    #[test]
    fn test_device_link_relay_message_encoding() {
        let msg = DeviceLinkRelayMessage {
            target_identity: "abc123def456".to_string(),
            sender_token: "tok-001".to_string(),
            payload: vec![0x01, 0x02, 0x03, 0xFF],
        };

        let encoded = encode_device_link_message(&msg);
        let decoded =
            decode_device_link_message(&encoded).expect("roundtrip decode should succeed");

        assert_eq!(decoded.target_identity, "abc123def456");
        assert_eq!(decoded.sender_token, "tok-001");
        assert_eq!(decoded.payload, vec![0x01, 0x02, 0x03, 0xFF]);
    }

    // @scenario: device_sync:Device link empty payload roundtrip
    #[test]
    fn test_device_link_relay_message_with_empty_payload() {
        let msg = DeviceLinkRelayMessage {
            target_identity: "identity-key".to_string(),
            sender_token: "token-empty".to_string(),
            payload: vec![],
        };

        let encoded = encode_device_link_message(&msg);
        let decoded =
            decode_device_link_message(&encoded).expect("empty payload roundtrip should succeed");

        assert_eq!(decoded.target_identity, "identity-key");
        assert_eq!(decoded.sender_token, "token-empty");
        assert!(decoded.payload.is_empty());
    }

    // @scenario: device_sync:Device link rejects invalid JSON
    #[test]
    fn test_device_link_relay_message_decode_invalid_data() {
        let invalid = b"not valid json";
        let result = decode_device_link_message(invalid);
        assert!(result.is_err());
    }

    // @scenario: device_sync:Claim payload serialization roundtrip
    #[test]
    fn test_claim_payload_roundtrip() {
        let payload = ClaimPayload {
            request: vec![1, 2, 3],
            response_code: "ABC123".to_string(),
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let parsed: ClaimPayload = serde_json::from_slice(&json).unwrap();
        assert_eq!(parsed.request, vec![1, 2, 3]);
        assert_eq!(parsed.response_code, "ABC123");
    }
}
