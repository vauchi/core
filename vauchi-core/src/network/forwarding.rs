// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Forwarding Hint Following
//!
//! When a relay offloads blobs to peer relays (federation), it sends
//! forwarding hints to the recipient client. This module handles parsing
//! those hints and fetching the offloaded blobs from the hinted relays.
//!
//! ## Signature Verification (Tracker #117)
//!
//! Forwarding hints may be signed by the relay's Ed25519 key. Clients
//! should call [`verify_hint_signature`] before following hints to ensure
//! they originate from the expected relay and have not been tampered with.

use std::collections::HashSet;

use subtle::ConstantTimeEq;
use thiserror::Error;

use super::message::{ForwardingHint, ForwardingHints, MessageEnvelope};

/// Errors from verifying forwarding hint signatures.
#[derive(Error, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HintVerificationError {
    #[error("unsigned")]
    Unsigned,

    #[error("missing signature")]
    MissingSignature,

    #[error("invalid key length: Ed25519 public key hex must be 64 chars")]
    InvalidKeyLength,

    #[error("relay key mismatch")]
    RelayKeyMismatch,

    #[error("invalid public key hex: {0}")]
    InvalidPublicKeyHex(String),

    #[error("invalid signature hex: {0}")]
    InvalidSignatureHex(String),

    #[error("signature verification failed")]
    VerificationFailed,
}

/// Filters out expired hints based on the current time.
pub fn filter_expired_hints(hints: &ForwardingHints, now_secs: u64) -> Vec<&ForwardingHint> {
    hints
        .hints
        .iter()
        .filter(|h| h.expires_at_secs > now_secs)
        .collect()
}

/// Deduplicates forwarding hints by blob_id, keeping the first occurrence.
pub fn deduplicate_hints(hints: &[ForwardingHint]) -> Vec<&ForwardingHint> {
    let mut seen = HashSet::new();
    hints
        .iter()
        .filter(|h| seen.insert(h.blob_id.as_str()))
        .collect()
}

/// Deduplicates received message envelopes by message_id.
pub fn deduplicate_envelopes(envelopes: Vec<MessageEnvelope>) -> Vec<MessageEnvelope> {
    let mut seen = HashSet::new();
    envelopes
        .into_iter()
        .filter(|e| seen.insert(e.message_id.clone()))
        .collect()
}

/// Groups forwarding hints by relay URL for batch fetching.
pub fn group_hints_by_relay<'a>(
    hints: &[&'a ForwardingHint],
) -> Vec<(&'a str, Vec<&'a ForwardingHint>)> {
    let mut groups: Vec<(&'a str, Vec<&'a ForwardingHint>)> = Vec::new();

    for hint in hints {
        if let Some(group) = groups.iter_mut().find(|(url, _)| *url == hint.relay_url) {
            group.1.push(hint);
        } else {
            groups.push((hint.relay_url.as_str(), vec![hint]));
        }
    }

    groups
}

/// Verifies the Ed25519 signature on forwarding hints (Tracker #117).
///
/// Returns `Ok(())` if the signature is valid and was produced by the
/// given `expected_relay_key` (32-byte Ed25519 public key, hex-encoded).
/// Returns `Err` with a description if verification fails.
///
/// If the hints are unsigned (no `relay_signing_key` or `signature`),
/// returns `Err("unsigned")`.
pub fn verify_hint_signature(
    hints: &ForwardingHints,
    expected_relay_key: &str,
) -> Result<(), HintVerificationError> {
    let relay_key_hex = hints
        .relay_signing_key
        .as_ref()
        .ok_or(HintVerificationError::Unsigned)?;
    let signature_hex = hints
        .signature
        .as_ref()
        .ok_or(HintVerificationError::MissingSignature)?;

    // Validate hex key lengths before constant-time comparison
    // (ct_eq on different-length slices short-circuits, leaking length)
    if relay_key_hex.len() != 64 || expected_relay_key.len() != 64 {
        return Err(HintVerificationError::InvalidKeyLength);
    }

    // Verify the signing key matches the expected relay
    if !bool::from(
        relay_key_hex
            .as_bytes()
            .ct_eq(expected_relay_key.as_bytes()),
    ) {
        return Err(HintVerificationError::RelayKeyMismatch);
    }

    let pk_bytes = hex::decode(relay_key_hex)
        .map_err(|e| HintVerificationError::InvalidPublicKeyHex(e.to_string()))?;
    let sig_bytes = hex::decode(signature_hex)
        .map_err(|e| HintVerificationError::InvalidSignatureHex(e.to_string()))?;

    let pk_array: [u8; 32] = pk_bytes
        .try_into()
        .map_err(|_| HintVerificationError::InvalidPublicKeyHex("not 32 bytes".to_string()))?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| HintVerificationError::InvalidSignatureHex("not 64 bytes".to_string()))?;

    let public_key = crate::crypto::signing::PublicKey::from_bytes(pk_array);
    let signature = crate::crypto::signing::Signature::from_bytes(sig_array);

    let canonical = hints.canonical_data();
    if public_key.verify(&canonical, &signature) {
        Ok(())
    } else {
        Err(HintVerificationError::VerificationFailed)
    }
}

// INLINE_TEST_REQUIRED: tests access private internals
#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::message::{
        ForwardingHint, ForwardingHints, MessageEnvelope, MessagePayload, PROTOCOL_VERSION,
    };

    fn make_hint(blob_id: &str, relay_url: &str, expires_at: u64) -> ForwardingHint {
        ForwardingHint {
            blob_id: blob_id.to_string(),
            relay_url: relay_url.to_string(),
            expires_at_secs: expires_at,
        }
    }

    fn make_unsigned_hints(hint_list: Vec<ForwardingHint>) -> ForwardingHints {
        ForwardingHints {
            hints: hint_list,
            relay_signing_key: None,
            signature: None,
        }
    }

    #[test]
    fn test_filter_expired_hints() {
        let hints = make_unsigned_hints(vec![
            make_hint("blob-1", "https://relay-a.test", 1000),
            make_hint("blob-2", "https://relay-b.test", 2000),
            make_hint("blob-3", "https://relay-a.test", 500),
        ]);

        let active = filter_expired_hints(&hints, 800);
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].blob_id, "blob-1");
        assert_eq!(active[1].blob_id, "blob-2");
    }

    #[test]
    fn test_filter_all_expired() {
        let hints = make_unsigned_hints(vec![
            make_hint("blob-1", "https://relay-a.test", 100),
            make_hint("blob-2", "https://relay-b.test", 200),
        ]);

        let active = filter_expired_hints(&hints, 300);
        assert!(active.is_empty());
    }

    #[test]
    fn test_filter_none_expired() {
        let hints = make_unsigned_hints(vec![
            make_hint("blob-1", "https://relay-a.test", 1000),
            make_hint("blob-2", "https://relay-b.test", 2000),
        ]);

        let active = filter_expired_hints(&hints, 0);
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_deduplicate_hints() {
        let hints = vec![
            make_hint("blob-1", "https://relay-a.test", 1000),
            make_hint("blob-1", "https://relay-b.test", 2000), // duplicate blob_id
            make_hint("blob-2", "https://relay-a.test", 1500),
        ];

        let deduped = deduplicate_hints(&hints);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].blob_id, "blob-1");
        assert_eq!(deduped[0].relay_url, "https://relay-a.test"); // first occurrence kept
        assert_eq!(deduped[1].blob_id, "blob-2");
    }

    #[test]
    fn test_deduplicate_envelopes() {
        let envelopes = vec![
            MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: "msg-1".to_string().into(),
                timestamp: 100,
                payload: MessagePayload::ForwardingHints(ForwardingHints {
                    hints: vec![],
                    relay_signing_key: None,
                    signature: None,
                }),
            },
            MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: "msg-1".to_string().into(), // duplicate
                timestamp: 200,
                payload: MessagePayload::ForwardingHints(ForwardingHints {
                    hints: vec![],
                    relay_signing_key: None,
                    signature: None,
                }),
            },
            MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: "msg-2".to_string().into(),
                timestamp: 300,
                payload: MessagePayload::ForwardingHints(ForwardingHints {
                    hints: vec![],
                    relay_signing_key: None,
                    signature: None,
                }),
            },
        ];

        let deduped = deduplicate_envelopes(envelopes);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].message_id, "msg-1");
        assert_eq!(deduped[1].message_id, "msg-2");
    }

    #[test]
    fn test_group_hints_by_relay() {
        let hints = [
            make_hint("blob-1", "https://relay-a.test", 1000),
            make_hint("blob-2", "https://relay-b.test", 1000),
            make_hint("blob-3", "https://relay-a.test", 1000),
            make_hint("blob-4", "https://relay-b.test", 1000),
            make_hint("blob-5", "https://relay-c.test", 1000),
        ];

        let hint_refs: Vec<&ForwardingHint> = hints.iter().collect();
        let groups = group_hints_by_relay(&hint_refs);

        assert_eq!(groups.len(), 3);

        // Find relay-a group
        let relay_a = groups
            .iter()
            .find(|(url, _)| *url == "https://relay-a.test")
            .unwrap();
        assert_eq!(relay_a.1.len(), 2);

        // Find relay-b group
        let relay_b = groups
            .iter()
            .find(|(url, _)| *url == "https://relay-b.test")
            .unwrap();
        assert_eq!(relay_b.1.len(), 2);

        // Find relay-c group
        let relay_c = groups
            .iter()
            .find(|(url, _)| *url == "https://relay-c.test")
            .unwrap();
        assert_eq!(relay_c.1.len(), 1);
    }

    #[test]
    fn test_empty_hints() {
        let hints = ForwardingHints {
            hints: vec![],
            relay_signing_key: None,
            signature: None,
        };
        let active = filter_expired_hints(&hints, 0);
        assert!(active.is_empty());

        let deduped = deduplicate_hints(&[]);
        assert!(deduped.is_empty());
    }

    #[test]
    fn test_forwarding_hints_serde_roundtrip() {
        let hints = make_unsigned_hints(vec![
            make_hint("blob-1", "https://relay-a.test", 1000),
            make_hint("blob-2", "https://relay-b.test", 2000),
        ]);

        let json = serde_json::to_string(&hints).unwrap();
        let deserialized: ForwardingHints = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.hints.len(), 2);
        assert_eq!(deserialized.hints[0].blob_id, "blob-1");
        assert_eq!(deserialized.hints[1].relay_url, "https://relay-b.test");
    }

    #[test]
    fn test_forwarding_hints_in_envelope() {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test-fwd-1".to_string().into(),
            timestamp: 1700000000,
            payload: MessagePayload::ForwardingHints(make_unsigned_hints(vec![make_hint(
                "blob-1",
                "https://relay-a.test",
                1000,
            )])),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let deserialized: MessageEnvelope = serde_json::from_str(&json).unwrap();

        match deserialized.payload {
            MessagePayload::ForwardingHints(fh) => {
                assert_eq!(fh.hints.len(), 1);
                assert_eq!(fh.hints[0].blob_id, "blob-1");
            }
            _ => panic!("Expected ForwardingHints variant"),
        }
    }

    // === Tracker #117: Signature Verification Tests ===

    #[test]
    fn test_verify_unsigned_hints_returns_error() {
        let hints = make_unsigned_hints(vec![make_hint("blob-1", "https://relay.test", 1000)]);
        let result = verify_hint_signature(&hints, "deadbeef");
        assert_eq!(result, Err(HintVerificationError::Unsigned));
    }

    #[test]
    fn test_verify_mismatched_relay_key_returns_error() {
        let hints = ForwardingHints {
            hints: vec![make_hint("blob-1", "https://relay.test", 1000)],
            relay_signing_key: Some("aa".repeat(32)),
            signature: Some("bb".repeat(64)),
        };
        let result = verify_hint_signature(&hints, &"cc".repeat(32));
        assert!(result.is_err(), "expected error");
        assert_eq!(result.unwrap_err(), HintVerificationError::RelayKeyMismatch);
    }

    #[test]
    fn test_canonical_data_is_order_independent() {
        let hints1 = make_unsigned_hints(vec![
            make_hint("blob-b", "https://relay-2.test", 2000),
            make_hint("blob-a", "https://relay-1.test", 1000),
        ]);
        let hints2 = make_unsigned_hints(vec![
            make_hint("blob-a", "https://relay-1.test", 1000),
            make_hint("blob-b", "https://relay-2.test", 2000),
        ]);
        assert_eq!(hints1.canonical_data(), hints2.canonical_data());
    }
}
