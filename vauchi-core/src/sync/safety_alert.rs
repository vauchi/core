// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Safety-alert payload — the signed, replay-protected envelope carried by
//! emergency-broadcast and duress alerts.
//!
//! Carried inside the Double Ratchet plaintext as [`VersionedPayload`]
//! version `0x04` (see `delta.rs`), so on the wire it is indistinguishable
//! from a card update (ADR-032). The version byte and this payload live
//! *inside* the ratchet-authenticated ciphertext, so a relay or network
//! observer can neither see nor flip them. The recipient routes on the
//! version byte after decryption and surfaces the alert.
//!
//! Why signed (not ratchet-AEAD only): the coercion threat model includes
//! device seizure, so a stolen ratchet state must not let an attacker forge
//! an alert *as* the victim or redirect one to a different recipient. The
//! Ed25519 signature binds the alert to a specific sender **and** recipient
//! (mirrors `CardDelta` / `ReciprocityConfirmPayload`). The random `nonce`
//! gives the receiver a value to run its existing replay check against, so
//! a captured blob cannot be replayed to re-trigger the alert. Design +
//! review: `2026-07-04-coercion-safety-alerts-never-received`.
//!
//! [`VersionedPayload`]: crate::sync::delta::VersionedPayload

use serde::{Deserialize, Serialize};

use crate::crypto::signing::{PublicKey, Signature};
use crate::identity::Identity;
use crate::network::GeoLocation;
use crate::sync::delta::DeltaError;

/// Domain separation for the safety-alert signature (prevents cross-context
/// signature reuse with card deltas / reciprocity confirmations).
const ALERT_DOMAIN: &[u8] = b"vauchi-sync-safety-alert-v1";

/// Fixed size of the `nonce || signature` prefix on the wire.
const PREFIX_LEN: usize = 32 + 64;

/// Which safety flow produced the alert. Carried inside the signed envelope
/// so the recipient can act appropriately (covert for duress, overt for
/// emergency); invisible on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertKind {
    /// Voluntary emergency broadcast.
    Emergency,
    /// Covert duress alert (ADR-032 — disguised as a card update on the
    /// sender's wire; the recipient still surfaces it).
    Duress,
}

/// Signed content of a safety alert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct AlertContent {
    kind: AlertKind,
    message: String,
    timestamp: u64,
    location: Option<GeoLocation>,
}

/// A safety-alert payload — [`VersionedPayload`] version `0x04`.
///
/// Wire form (after the `0x04` version byte is stripped):
/// `nonce(32) || signature(64) || content_bytes(rest)`, where
/// `content_bytes` is the serialized [`AlertContent`]. The Ed25519 signature
/// covers `ALERT_DOMAIN || sender_pk || recipient_pk || nonce ||
/// content_bytes`. [`verify`](Self::verify) signs over the *received*
/// `content_bytes`, so no canonical re-serialization is required.
///
/// [`VersionedPayload`]: crate::sync::delta::VersionedPayload
#[derive(Debug, Clone)]
pub struct SafetyAlertPayload {
    nonce: [u8; 32],
    signature: [u8; 64],
    content: AlertContent,
    /// The exact bytes the signature was computed over (the wire content).
    content_bytes: Vec<u8>,
}

impl SafetyAlertPayload {
    /// Build and sign a new safety alert bound to `recipient_pk`.
    pub fn new(
        kind: AlertKind,
        message: String,
        timestamp: u64,
        location: Option<GeoLocation>,
        nonce: [u8; 32],
        identity: &Identity,
        recipient_pk: &[u8; 32],
    ) -> Result<Self, DeltaError> {
        let content = AlertContent {
            kind,
            message,
            timestamp,
            location,
        };
        let content_bytes = serde_json::to_vec(&content)
            .map_err(|e| DeltaError::InvalidPayload(format!("alert content: {e}")))?;
        let signable = Self::signable(
            identity.signing_public_key(),
            recipient_pk,
            &nonce,
            &content_bytes,
        );
        let signature = *identity.sign(&signable).as_bytes();
        Ok(Self {
            nonce,
            signature,
            content,
            content_bytes,
        })
    }

    /// Encode to wire (without the `0x04` version byte prefix — use
    /// `VersionedPayload::encode_alert`).
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(PREFIX_LEN + self.content_bytes.len());
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&self.signature);
        buf.extend_from_slice(&self.content_bytes);
        buf
    }

    /// Decode from wire (after the version byte has been stripped).
    ///
    /// Structural decode only — the caller must [`verify`](Self::verify) the
    /// signature before acting on the alert.
    pub fn decode(data: &[u8]) -> Result<Self, DeltaError> {
        if data.len() < PREFIX_LEN {
            return Err(DeltaError::InvalidPayload(format!(
                "safety alert: expected >= {PREFIX_LEN} bytes, got {}",
                data.len()
            )));
        }
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&data[..32]);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&data[32..PREFIX_LEN]);
        let content_bytes = data[PREFIX_LEN..].to_vec();
        let content: AlertContent = serde_json::from_slice(&content_bytes)
            .map_err(|e| DeltaError::InvalidPayload(format!("alert content: {e}")))?;
        Ok(Self {
            nonce,
            signature,
            content,
            content_bytes,
        })
    }

    /// Verify the Ed25519 signature binds this alert to `sender_pk` and
    /// `recipient_pk` (and to its exact content). Signs over the *received*
    /// content bytes, so canonical re-serialization is not required.
    #[must_use]
    pub fn verify(&self, sender_pk: &[u8; 32], recipient_pk: &[u8; 32]) -> bool {
        let signable = Self::signable(sender_pk, recipient_pk, &self.nonce, &self.content_bytes);
        let pk = PublicKey::from_bytes(*sender_pk);
        let sig = Signature::from_bytes(self.signature);
        pk.verify(&signable, &sig)
    }

    /// The alert flow (emergency vs duress).
    pub fn kind(&self) -> AlertKind {
        self.content.kind
    }

    /// The alert message.
    pub fn message(&self) -> &str {
        &self.content.message
    }

    /// When the alert was created (unix seconds).
    pub fn timestamp(&self) -> u64 {
        self.content.timestamp
    }

    /// Optional sender location.
    pub fn location(&self) -> Option<&GeoLocation> {
        self.content.location.as_ref()
    }

    /// The replay nonce (fed to the receiver's replay check).
    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    fn signable(
        sender_pk: &[u8; 32],
        recipient_pk: &[u8; 32],
        nonce: &[u8; 32],
        content_bytes: &[u8],
    ) -> Vec<u8> {
        let mut msg = Vec::with_capacity(ALERT_DOMAIN.len() + 96 + content_bytes.len());
        msg.extend_from_slice(ALERT_DOMAIN);
        msg.extend_from_slice(sender_pk);
        msg.extend_from_slice(recipient_pk);
        msg.extend_from_slice(nonce);
        msg.extend_from_slice(content_bytes);
        msg
    }
}
