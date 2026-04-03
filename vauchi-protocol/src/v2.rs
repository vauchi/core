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
}

/// Standard V2 response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}
