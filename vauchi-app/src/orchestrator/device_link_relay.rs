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

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use vauchi_core::sleeper::Sleeper;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use thiserror::Error;
use vauchi_core::network::HttpTransport;

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

// Transports are now built by the caller via `Vauchi::build_relay_transport`
// so that device-link requests can flow through OHTTP once the calling app
// has bootstrapped a gateway key. See problem record
// `_private/docs/problems/2026-04-17-ohttp-allow-direct-fallback/`.

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

/// Create a device-link offer for the existing device (initiator).
///
/// Posts an `exchange_offer` with the identity info and returns the
/// broker code that gets embedded in the QR. This is the first step
/// of the initiator's relay flow; the second step is
/// [`poll_for_claim`].
pub fn create_offer(
    transport: &HttpTransport,
    identity_id: &str,
    timeout_secs: u64,
) -> Result<String, DeviceLinkError> {
    transport
        .exchange_offer(&BASE64.encode(identity_id.as_bytes()), Some(timeout_secs))
        .map_err(|e| DeviceLinkError::OfferFailed(e.to_string()))
}

/// Poll a previously-created offer until the new device claims it.
///
/// Used by the orchestrator's cycle thread on the initiator side.
/// Loops calling `exchange_complete(code)` once per second until one of:
///
/// - `Ok(Some(...))` — peer claimed; returns `(request_payload, response_code)`
/// - `Instant::now() >= deadline` — `Err(RequestTimeout)`
/// - `cancel.load(Relaxed)` — `Err(RequestTimeout)` (cancellation surfaces
///   as the same timeout error today; the orchestrator tags it semantically
///   based on which condition tripped first)
/// - transport error — `Err(Network)` propagated immediately
///
/// The 1-second poll cadence is the granularity at which cancel and
/// expiry are observed; both usually surface within ~1 s.
pub fn poll_for_claim(
    transport: &HttpTransport,
    code: &str,
    deadline: Instant,
    cancel: &AtomicBool,
    sleeper: &dyn Sleeper,
) -> Result<(Vec<u8>, String), DeviceLinkError> {
    loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            return Err(DeviceLinkError::RequestTimeout);
        }
        match transport.exchange_complete(code) {
            Ok(Some(claim_b64)) => {
                let claim_bytes = BASE64
                    .decode(&claim_b64)
                    .map_err(|e| DeviceLinkError::Network(format!("decode: {e}")))?;
                let claim: ClaimPayload =
                    serde_json::from_slice(&claim_bytes).map_err(DeviceLinkError::DecodeFailed)?;
                return Ok((claim.request, claim.response_code));
            }
            Ok(None) => {
                sleeper.sleep(Duration::from_secs(1));
            }
            Err(e) => return Err(DeviceLinkError::Network(e.to_string())),
        }
    }
}

/// Claim the existing device's offer and post our encrypted request
/// (responder side). Returns the `response_code` for the return channel
/// the orchestrator polls via [`poll_for_response`].
///
/// Two HTTP roundtrips: `exchange_offer("")` to allocate the return
/// channel, then `exchange_claim(message.sender_token, …)` to deposit
/// the encrypted request alongside the return code.
pub fn claim_and_send_request(
    transport: &HttpTransport,
    message: &DeviceLinkRelayMessage,
    timeout_secs: u64,
) -> Result<String, DeviceLinkError> {
    let response_code = transport
        .exchange_offer(&BASE64.encode(b""), Some(timeout_secs))
        .map_err(|e| DeviceLinkError::OfferFailed(e.to_string()))?;

    let claim_payload = ClaimPayload {
        request: message.payload.clone(),
        response_code: response_code.clone(),
    };
    let claim_json = serde_json::to_vec(&claim_payload)
        .map_err(|e| DeviceLinkError::ClaimFailed(e.to_string()))?;

    transport
        .exchange_claim(&message.sender_token, &BASE64.encode(&claim_json))
        .map_err(|e| DeviceLinkError::ClaimFailed(e.to_string()))?;

    Ok(response_code)
}

/// Poll a previously-claimed return channel for the initiator's
/// response (responder side). Same poll/cancel/deadline shape as
/// [`poll_for_claim`]; returns the raw response bytes.
pub fn poll_for_response(
    transport: &HttpTransport,
    response_code: &str,
    deadline: Instant,
    cancel: &AtomicBool,
    sleeper: &dyn Sleeper,
) -> Result<Vec<u8>, DeviceLinkError> {
    loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            return Err(DeviceLinkError::ResponseTimeout);
        }
        match transport.exchange_complete(response_code) {
            Ok(Some(response_b64)) => {
                return BASE64
                    .decode(&response_b64)
                    .map_err(|e| DeviceLinkError::Network(format!("decode: {e}")));
            }
            Ok(None) => {
                sleeper.sleep(Duration::from_secs(1));
            }
            Err(e) => return Err(DeviceLinkError::Network(e.to_string())),
        }
    }
}

/// Send a device link request via relay and wait for the response
/// (responder; legacy single-call API). Now a thin shim over
/// [`claim_and_send_request`] + [`poll_for_response`] with a
/// never-tripped cancel flag.
pub fn send_and_receive(
    transport: &HttpTransport,
    message: &DeviceLinkRelayMessage,
    timeout_secs: u64,
) -> Result<Vec<u8>, DeviceLinkError> {
    let response_code = claim_and_send_request(transport, message, timeout_secs)?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let never_cancel = AtomicBool::new(false);
    let sleeper = vauchi_core::sleeper::SystemSleeper::shared();
    poll_for_response(
        transport,
        &response_code,
        deadline,
        &never_cancel,
        &*sleeper,
    )
}

/// Listen for an incoming device link request via relay (initiator;
/// legacy single-call API). Now a thin shim over [`create_offer`] +
/// [`poll_for_claim`] with a never-tripped cancel flag.
///
/// Returns `(code, payload, sender_token)` — `code` is the exchange
/// broker code (embedded in the QR) and `sender_token` is the return
/// channel's response_code that the initiator uses to send its reply.
pub fn create_offer_and_listen(
    transport: &HttpTransport,
    identity_id: &str,
    timeout_secs: u64,
) -> Result<(String, Vec<u8>, String), DeviceLinkError> {
    let code = create_offer(transport, identity_id, timeout_secs)?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let never_cancel = AtomicBool::new(false);
    let sleeper = vauchi_core::sleeper::SystemSleeper::shared();
    let (payload, sender_token) =
        poll_for_claim(transport, &code, deadline, &never_cancel, &*sleeper)?;
    Ok((code, payload, sender_token))
}

/// Listen for an incoming device link request (legacy API adapter).
///
/// Wraps `create_offer_and_listen` — the caller already has the code from QR
/// generation, so this just polls for the claim. But since the exchange broker
/// requires the code to be generated by `exchange_offer`, we generate a new
/// one here and return the payload + sender_token.
pub fn listen_for_request(
    transport: &HttpTransport,
    identity_id: &str,
    timeout_secs: u64,
) -> Result<(Vec<u8>, String), DeviceLinkError> {
    let (_code, payload, sender_token) =
        create_offer_and_listen(transport, identity_id, timeout_secs)?;
    Ok((payload, sender_token))
}

/// Send a device link response back via relay.
///
/// Used by the **existing device** (initiator) to claim the return channel
/// created by the new device, depositing the encrypted response.
pub fn send_response(
    transport: &HttpTransport,
    sender_token: &str,
    response_payload: Vec<u8>,
) -> Result<(), DeviceLinkError> {
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
