// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 bilateral registry activation payloads (ADR-064 Amendment 2026-07-25).
//!
//! [`RegistryPushPayload`] (version `0x05`) carries the sender's
//! identity-signed `RegistryBroadcast` to a contact over the existing
//! ratcheted channel; [`RegistryAckPayload`] (version `0x06`) confirms the
//! received registry version and optionally carries the responder's own
//! broadcast back, completing the responder half of the handshake in one
//! round-trip.
//!
//! Both payloads are structural containers. Authenticity comes from two
//! layers the payloads do not duplicate: the ratchet session authenticates
//! the channel, and the carried broadcast's own Ed25519 signature is
//! verified against the contact's known identity key at persist time
//! (`DeviceStore::save_contact_device_registry` also enforces monotonic
//! versions). Decode validates structure only — size ceiling and
//! broadcast-shape parse (DC-01) — and fails closed on anything else.

use crate::identity::RegistryBroadcast;
use crate::sync::delta::DeltaError;

/// Ceiling for a carried broadcast, aligned with the genesis envelope's
/// registry ceiling so a broadcast that fits one path fits the other.
pub const MAX_BROADCAST_JSON_BYTES: usize = 4096;

/// Push nonce length on the wire.
const NONCE_LEN: usize = 32;

/// Ack prefix: `push_nonce(32) || acked_version(8) || echo_flag(1)`.
const ACK_PREFIX_LEN: usize = NONCE_LEN + 8 + 1;

fn validate_broadcast_json(bytes: &[u8]) -> Result<(), DeltaError> {
    if bytes.len() > MAX_BROADCAST_JSON_BYTES {
        return Err(DeltaError::InvalidPayload(format!(
            "registry broadcast: {} bytes exceeds ceiling {MAX_BROADCAST_JSON_BYTES}",
            bytes.len()
        )));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| DeltaError::InvalidPayload("registry broadcast: not UTF-8".into()))?;
    RegistryBroadcast::from_json(text)
        .map_err(|e| DeltaError::InvalidPayload(format!("registry broadcast: {e:?}")))?;
    Ok(())
}

/// A registry push — [`VersionedPayload`] version `0x05`.
///
/// Wire form (after the version byte): `push_nonce(32) || broadcast_json`.
///
/// [`VersionedPayload`]: crate::sync::delta::VersionedPayload
#[derive(Debug, Clone)]
pub struct RegistryPushPayload {
    push_nonce: [u8; NONCE_LEN],
    broadcast_json: Vec<u8>,
}

impl RegistryPushPayload {
    /// Build a push, enforcing the broadcast ceiling and shape.
    pub fn new(push_nonce: [u8; NONCE_LEN], broadcast_json: Vec<u8>) -> Result<Self, DeltaError> {
        validate_broadcast_json(&broadcast_json)?;
        Ok(Self {
            push_nonce,
            broadcast_json,
        })
    }

    /// Encode to wire (without the `0x05` version byte — use
    /// `VersionedPayload::encode_registry_push`).
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(NONCE_LEN + self.broadcast_json.len());
        buf.extend_from_slice(&self.push_nonce);
        buf.extend_from_slice(&self.broadcast_json);
        buf
    }

    /// Decode from wire (after the version byte has been stripped).
    pub fn decode(data: &[u8]) -> Result<Self, DeltaError> {
        if data.len() < NONCE_LEN {
            return Err(DeltaError::PayloadTooShort);
        }
        let mut push_nonce = [0u8; NONCE_LEN];
        push_nonce.copy_from_slice(&data[..NONCE_LEN]);
        let broadcast_json = data[NONCE_LEN..].to_vec();
        validate_broadcast_json(&broadcast_json)?;
        Ok(Self {
            push_nonce,
            broadcast_json,
        })
    }

    /// Correlation nonce echoed back by the matching ack.
    pub fn push_nonce(&self) -> &[u8; NONCE_LEN] {
        &self.push_nonce
    }

    /// The carried identity-signed broadcast, as raw JSON bytes.
    pub fn broadcast_json(&self) -> &[u8] {
        &self.broadcast_json
    }
}

/// A registry ack — [`VersionedPayload`] version `0x06`.
///
/// Wire form (after the version byte):
/// `push_nonce(32) || acked_version(8 BE) || echo_flag(1) || broadcast_json?`
/// where `broadcast_json` is present exactly when `echo_flag == 1` (the
/// responder's own broadcast riding the ack).
///
/// [`VersionedPayload`]: crate::sync::delta::VersionedPayload
#[derive(Debug, Clone)]
pub struct RegistryAckPayload {
    push_nonce: [u8; NONCE_LEN],
    acked_version: u64,
    broadcast_json: Option<Vec<u8>>,
}

impl RegistryAckPayload {
    /// Build an ack, enforcing the echo ceiling and shape when present.
    pub fn new(
        push_nonce: [u8; NONCE_LEN],
        acked_version: u64,
        broadcast_json: Option<Vec<u8>>,
    ) -> Result<Self, DeltaError> {
        if let Some(bytes) = &broadcast_json {
            validate_broadcast_json(bytes)?;
        }
        Ok(Self {
            push_nonce,
            acked_version,
            broadcast_json,
        })
    }

    /// Encode to wire (without the `0x06` version byte — use
    /// `VersionedPayload::encode_registry_ack`).
    pub fn encode(&self) -> Vec<u8> {
        let echo = self.broadcast_json.as_deref().unwrap_or(&[]);
        let mut buf = Vec::with_capacity(ACK_PREFIX_LEN + echo.len());
        buf.extend_from_slice(&self.push_nonce);
        buf.extend_from_slice(&self.acked_version.to_be_bytes());
        buf.push(u8::from(self.broadcast_json.is_some()));
        buf.extend_from_slice(echo);
        buf
    }

    /// Decode from wire (after the version byte has been stripped).
    pub fn decode(data: &[u8]) -> Result<Self, DeltaError> {
        if data.len() < ACK_PREFIX_LEN {
            return Err(DeltaError::PayloadTooShort);
        }
        let mut push_nonce = [0u8; NONCE_LEN];
        push_nonce.copy_from_slice(&data[..NONCE_LEN]);
        let mut version_bytes = [0u8; 8];
        version_bytes.copy_from_slice(&data[NONCE_LEN..NONCE_LEN + 8]);
        let acked_version = u64::from_be_bytes(version_bytes);
        let echo_flag = data[ACK_PREFIX_LEN - 1];
        let rest = &data[ACK_PREFIX_LEN..];
        let broadcast_json = match (echo_flag, rest.is_empty()) {
            (0, true) => None,
            (1, false) => {
                validate_broadcast_json(rest)?;
                Some(rest.to_vec())
            }
            // Flag and body must agree — a flag promising absent bytes (or
            // trailing junk behind flag 0) is malformed, not tolerable.
            _ => {
                return Err(DeltaError::InvalidPayload(
                    "registry ack: echo flag and body disagree".into(),
                ));
            }
        };
        Ok(Self {
            push_nonce,
            acked_version,
            broadcast_json,
        })
    }

    /// Correlation nonce of the push this ack answers.
    pub fn push_nonce(&self) -> &[u8; NONCE_LEN] {
        &self.push_nonce
    }

    /// The registry version the receiver persisted.
    pub fn acked_version(&self) -> u64 {
        self.acked_version
    }

    /// The responder's own broadcast riding the ack, if any.
    pub fn broadcast_json(&self) -> Option<&[u8]> {
        self.broadcast_json.as_deref()
    }
}
