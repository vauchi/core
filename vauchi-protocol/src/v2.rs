// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared V2 HTTP API request/response types.
//!
//! These types are the wire format between `vauchi-core` (client) and
//! `vauchi-relay` (server) for the V2 REST API. Both sides serialize and
//! deserialize with `serde_json`, so every type derives both `Serialize`
//! and `Deserialize`.

use serde::{Deserialize, Serialize};

/// V2 send request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2SendRequest {
    pub recipient_id: String,
    /// Base64-encoded ciphertext.
    pub ciphertext: String,
}

/// V2 fetch request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2FetchRequest {
    pub mailbox_tokens: Vec<String>,
}

/// V2 acknowledge request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2AckRequest {
    pub recipient_id: String,
    pub blob_id: String,
}

/// V2 register mailbox tokens request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RegisterRequest {
    pub mailbox_tokens: Vec<String>,
}

/// V2 purge request body.
///
/// Purge is destructive — requires Ed25519 signature over
/// `public_key || purge_token || timestamp`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2PurgeRequest {
    pub recipient_id: String,
    /// Hex-encoded Ed25519 public key (32 bytes).
    pub public_key: String,
    /// Hex-encoded purge token (32 bytes).
    pub purge_token: String,
    /// Hex-encoded Ed25519 signature (64 bytes).
    pub signature: String,
    /// Unix timestamp (must be within 60s of server time).
    pub timestamp: u64,
}

/// V2 exchange offer request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeOfferRequest {
    pub payload: String,
    pub expires_secs: Option<u64>,
}

/// V2 exchange claim request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeClaimRequest {
    pub code: String,
    pub response: String,
}

/// V2 exchange complete request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeCompleteRequest {
    pub code: String,
}

/// A fetched blob from the relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchedBlob {
    pub blob_id: String,
    /// Base64-encoded ciphertext.
    pub ciphertext: String,
    pub created_at: u64,
    /// Mailbox token the blob arrived for. Returned to the recipient
    /// (who already knows token→contact via local registration) so the
    /// receive loop can route in O(1) instead of brute-forcing every
    /// contact's ratchet. Privacy-neutral: the relay already has the
    /// token, the recipient computed it locally.
    ///
    /// `None` for older relays that have not deployed this field;
    /// clients fall back to brute-force routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mailbox_token: Option<String>,
}

/// V2 recovery proof store request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryStoreRequest {
    /// Hex-encoded hash of the old public key (32 bytes = 64 hex chars).
    pub key_hash: String,
    /// Base64-encoded recovery proof data (opaque to relay, max 4 KiB).
    pub proof_data: String,
}

/// V2 recovery proof query request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryQueryRequest {
    /// Hex-encoded key hashes to look up (max 50).
    pub key_hashes: Vec<String>,
}

/// A single recovery proof entry in a query response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryProof {
    pub key_hash: String,
    /// Base64-encoded proof data.
    pub proof_data: String,
    pub created_at: u64,
    pub expires_at: u64,
}

// ── Guardian Storage ──────────────────────────────────────────────

/// A single encrypted guardian entry (opaque to the relay).
///
/// Each entry is a sealed-box encrypted guardian token. The relay cannot
/// read contents, identify guardians, or learn anything beyond the entry count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianEntry {
    /// Base64-encoded encrypted blob (sealed-box: ephemeral X25519 + XChaCha20-Poly1305).
    pub data: String,
}

/// V2 guardian store request body.
///
/// Atomically replaces all guardian entries for a given hash.
/// Removing a guardian = re-upload without their entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianStoreRequest {
    /// Hex-encoded hash of `designator_pk || "guardians"` (64 hex chars = 32 bytes).
    pub guardian_hash: String,
    /// Encrypted entries (one per guardian, max 10).
    pub entries: Vec<V2GuardianEntry>,
}

/// V2 guardian query request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianQueryRequest {
    /// Hex-encoded guardian hash to look up (64 hex chars = 32 bytes).
    pub guardian_hash: String,
}

/// V2 guardian delete request body.
///
/// Deletes all guardian entries for a hash (identity purge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianDeleteRequest {
    /// Hex-encoded guardian hash to delete (64 hex chars = 32 bytes).
    pub guardian_hash: String,
}

/// Standard V2 response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct V2Response {
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub blob_id: Option<String>,
    #[serde(default)]
    pub blobs: Option<Vec<FetchedBlob>>,
    #[serde(default)]
    pub acknowledged: Option<bool>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub payload: Option<String>,
    #[serde(default)]
    pub response: Option<String>,
    /// Number of blobs deleted by a purge request.
    #[serde(default)]
    pub blobs_deleted: Option<usize>,
    /// Recovery proofs returned by a query.
    #[serde(default)]
    pub proofs: Option<Vec<V2RecoveryProof>>,
    /// Guardian entries returned by a query.
    #[serde(default)]
    pub guardians: Option<Vec<V2GuardianEntry>>,
}

impl V2Response {
    /// Create a response with the given status and all optional fields set to `None`.
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            error: None,
            blob_id: None,
            blobs: None,
            acknowledged: None,
            code: None,
            payload: None,
            response: None,
            blobs_deleted: None,
            proofs: None,
            guardians: None,
        }
    }
}
