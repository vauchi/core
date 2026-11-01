// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared V2 HTTP API request/response types.
//!
//! These types are the wire format between `vauchi-core` (client) and
//! `vauchi-relay` (server) for the V2 REST API. Both sides serialize and
//! deserialize with `serde_json`, so every type derives both `Serialize`
//! and `Deserialize`.
//!
//! Deserialization rejects oversized field values as a defence-in-depth
//! layer alongside relay validation. Transport-level body limits remain
//! responsible for bounding input buffering.

use serde::{Deserialize, Serialize};

mod bounded;
pub mod bounds;

// =========================================================================
// Size limits
// =========================================================================

/// Maximum length of a hex-encoded 32-byte identifier
/// (recipient_id, mailbox token, key_hash, public_key, designator_pk, ...).
const MAX_ID_HEX_LEN: usize = 64;

/// Maximum length of a hex-encoded Ed25519 signature (64 bytes = 128 hex chars).
const MAX_SIGNATURE_HEX_LEN: usize = 128;

/// Maximum number of mailbox tokens per request.
pub const MAX_MAILBOX_TOKENS: usize = 1900;

/// Maximum base64-encoded ciphertext length (256 KiB; covers 192 KiB decoded).
const MAX_CIPHERTEXT_B64_LEN: usize = 256 * 1024;

/// Maximum base64-encoded recovery proof data length
/// (8 KiB covers the 4 KiB raw limit with base64 overhead and margin).
const MAX_PROOF_DATA_B64_LEN: usize = 8192;

/// Maximum number of key hashes per recovery query.
pub const MAX_RECOVERY_QUERY_HASHES: usize = 50;

/// Maximum number of guardian entries per store request.
pub const MAX_GUARDIAN_ENTRIES: usize = 10;

/// Maximum base64-encoded guardian entry length
/// (512 bytes covers the 256-byte raw limit with base64 overhead and margin).
const MAX_GUARDIAN_ENTRY_B64_LEN: usize = 512;

/// Maximum base64-encoded origin-device hint length. The hint carries
/// `nonce(24) || XChaCha20Poly1305(sender_device_id[32])` = 72 raw bytes
/// (96 base64 chars); 128 leaves margin without admitting an oversized field
/// (F4 origin-device hint design, 2026-07-27).
const MAX_ORIGIN_HINT_B64_LEN: usize = 128;

/// Maximum generic string length (1 MiB; generous upper bound for status/error text).
const MAX_STRING_LEN: usize = 1024 * 1024;

/// Maximum number of recovery proofs in a query response.
const MAX_RECOVERY_PROOFS_LEN: usize = 50;

/// Maximum number of guardian entries in a query response.
const MAX_GUARDIANS_LEN: usize = 10;

// =========================================================================
// Request / response types
// =========================================================================

/// V2 send request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2SendRequest {
    #[serde(deserialize_with = "bounded::deserialize_recipient_id")]
    pub recipient_id: String,
    /// Base64-encoded ciphertext.
    #[serde(deserialize_with = "bounded::deserialize_ciphertext")]
    pub ciphertext: String,
    /// Base64-encoded opaque origin-device hint (`nonce || AEAD(sender_device_id)`,
    /// encrypted to the recipient). Lets the receiver select the sender's
    /// device-pair ratchet without the transport learning the sender edge.
    ///
    /// `None` for older clients that predate this field; the receiver then
    /// falls back to the legacy `[0;32]` sender-device path. The relay stores
    /// and returns it verbatim, never inspecting it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "bounded::deserialize_optional_origin_hint"
    )]
    pub origin_hint: Option<String>,
}

/// V2 fetch request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2FetchRequest {
    #[serde(deserialize_with = "bounded::deserialize_mailbox_tokens")]
    pub mailbox_tokens: Vec<String>,
}

/// V2 acknowledge request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2AckRequest {
    #[serde(deserialize_with = "bounded::deserialize_recipient_id")]
    pub recipient_id: String,
    #[serde(deserialize_with = "bounded::deserialize_blob_id")]
    pub blob_id: String,
}

/// V2 register mailbox tokens request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RegisterRequest {
    #[serde(deserialize_with = "bounded::deserialize_mailbox_tokens")]
    pub mailbox_tokens: Vec<String>,
}

/// V2 purge request body: an Ed25519 signature over
/// `public_key || purge_token || timestamp`. It names no mailbox; the relay
/// erases the one addressed by `public_key`, since a body-chosen mailbox let
/// any key pair empty any mailbox whose token it held (#944).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2PurgeRequest {
    /// Hex-encoded Ed25519 public key (32 bytes).
    #[serde(deserialize_with = "bounded::deserialize_public_key")]
    pub public_key: String,
    /// Hex-encoded purge token (32 bytes).
    #[serde(deserialize_with = "bounded::deserialize_purge_token")]
    pub purge_token: String,
    /// Hex-encoded Ed25519 signature (64 bytes).
    #[serde(deserialize_with = "bounded::deserialize_signature")]
    pub signature: String,
    /// Unix timestamp (must be within 60s of server time).
    pub timestamp: u64,
    /// Also erase the guardian set at `SHA-256(public_key || "guardians")`.
    /// Covered by the signature; absent means keep it, which is all an old
    /// client could ask for.
    #[serde(default, skip_serializing_if = "bounded::is_false")]
    pub erase_guardian_set: bool,
}

/// V2 exchange offer request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeOfferRequest {
    #[serde(deserialize_with = "bounded::deserialize_exchange_payload")]
    pub payload: String,
    pub expires_secs: Option<u64>,
}

/// V2 exchange claim request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeClaimRequest {
    #[serde(deserialize_with = "bounded::deserialize_exchange_code")]
    pub code: String,
    #[serde(deserialize_with = "bounded::deserialize_exchange_response")]
    pub response: String,
}

/// V2 exchange complete request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeCompleteRequest {
    #[serde(deserialize_with = "bounded::deserialize_exchange_code")]
    pub code: String,
}

/// A fetched blob from the relay.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchedBlob {
    #[serde(deserialize_with = "bounded::deserialize_blob_id")]
    pub blob_id: String,
    /// Base64-encoded ciphertext.
    #[serde(deserialize_with = "bounded::deserialize_ciphertext")]
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "bounded::deserialize_optional_mailbox_token"
    )]
    pub mailbox_token: Option<String>,
    /// Base64-encoded opaque origin-device hint the sender attached (see
    /// [`V2SendRequest::origin_hint`]). Returned verbatim so the receiver can
    /// resolve the sender device before decrypting. `None` for blobs sent by
    /// older clients or relayed by older relays.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "bounded::deserialize_optional_origin_hint"
    )]
    pub origin_hint: Option<String>,
}

/// V2 recovery proof store request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryStoreRequest {
    /// Hex-encoded hash of the old public key (32 bytes = 64 hex chars).
    #[serde(deserialize_with = "bounded::deserialize_hex_hash")]
    pub key_hash: String,
    /// Base64-encoded recovery proof data (opaque to relay, max 4 KiB).
    #[serde(deserialize_with = "bounded::deserialize_proof_data")]
    pub proof_data: String,
    /// Hex-encoded Ed25519 public key of the identity uploading the proof,
    /// so its purge can erase the proof. Proofs without an owner expire.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "bounded::deserialize_optional_public_key"
    )]
    pub owner_pk: Option<String>,
    /// Unix seconds, signed with the owner key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    /// Hex-encoded Ed25519 signature by `owner_pk` over the upload.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "bounded::deserialize_optional_signature"
    )]
    pub signature: Option<String>,
}

/// V2 recovery proof query request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryQueryRequest {
    /// Hex-encoded key hashes to look up (max 50).
    #[serde(deserialize_with = "bounded::deserialize_key_hashes")]
    pub key_hashes: Vec<String>,
}

/// A single recovery proof entry in a query response.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryProof {
    #[serde(deserialize_with = "bounded::deserialize_hex_hash")]
    pub key_hash: String,
    /// Base64-encoded proof data.
    #[serde(deserialize_with = "bounded::deserialize_proof_data")]
    pub proof_data: String,
    pub created_at: u64,
    pub expires_at: u64,
}

// ── Guardian Storage ──────────────────────────────────────────────

/// A single encrypted guardian entry (opaque to the relay).
///
/// Each entry is a sealed-box encrypted guardian token. The relay cannot
/// read contents, identify guardians, or learn anything beyond the entry count.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianEntry {
    /// Base64-encoded encrypted blob (sealed-box: ephemeral X25519 + XChaCha20-Poly1305).
    #[serde(deserialize_with = "bounded::deserialize_guardian_entry_data")]
    pub data: String,
}

/// V2 guardian store request body.
///
/// Atomically replaces all guardian entries for a given hash.
/// Removing a guardian = re-upload without their entry.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianStoreRequest {
    /// Hex-encoded hash of `designator_pk || "guardians"` (64 hex chars = 32 bytes).
    #[serde(deserialize_with = "bounded::deserialize_hex_hash")]
    pub guardian_hash: String,
    /// Encrypted entries (one per guardian, max 10).
    #[serde(deserialize_with = "bounded::deserialize_guardian_entries")]
    pub entries: Vec<V2GuardianEntry>,
    /// Hex-encoded Ed25519 designator public key (64 hex chars). The relay
    /// requires `SHA-256(designator_pk || "guardians") == guardian_hash`,
    /// proving the caller owns the identity the hash derives from.
    #[serde(deserialize_with = "bounded::deserialize_designator_pk")]
    pub designator_pk: String,
    /// Unix seconds; the relay rejects requests outside ±60s.
    pub timestamp: u64,
    /// Hex-encoded Ed25519 signature (128 hex chars) over
    /// `domain || designator_pk || guardian_hash || timestamp_be`.
    #[serde(deserialize_with = "bounded::deserialize_signature")]
    pub signature: String,
}

/// V2 guardian query request body.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianQueryRequest {
    /// Hex-encoded guardian hash to look up (64 hex chars = 32 bytes).
    #[serde(deserialize_with = "bounded::deserialize_hex_hash")]
    pub guardian_hash: String,
}

/// V2 guardian delete request body.
///
/// Deletes all guardian entries for a hash (identity purge).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianDeleteRequest {
    /// Hex-encoded guardian hash to delete (64 hex chars = 32 bytes).
    #[serde(deserialize_with = "bounded::deserialize_hex_hash")]
    pub guardian_hash: String,
    /// Hex-encoded Ed25519 designator public key (64 hex chars). The relay
    /// requires `SHA-256(designator_pk || "guardians") == guardian_hash`,
    /// proving the caller owns the identity the hash derives from.
    #[serde(deserialize_with = "bounded::deserialize_designator_pk")]
    pub designator_pk: String,
    /// Unix seconds; the relay rejects requests outside ±60s.
    pub timestamp: u64,
    /// Hex-encoded Ed25519 signature (128 hex chars) over
    /// `domain || designator_pk || guardian_hash || timestamp_be`.
    #[serde(deserialize_with = "bounded::deserialize_signature")]
    pub signature: String,
}

/// Standard V2 response envelope.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct V2Response {
    #[serde(deserialize_with = "bounded::deserialize_status")]
    pub status: String,
    #[serde(default, deserialize_with = "bounded::deserialize_optional_error")]
    pub error: Option<String>,
    #[serde(default, deserialize_with = "bounded::deserialize_optional_blob_id")]
    pub blob_id: Option<String>,
    #[serde(default)]
    pub blobs: Option<Vec<FetchedBlob>>,
    /// `true` when `/v2/fetch` capped this page under the OHTTP forward limit
    /// and more blobs remain; the client re-fetches after ACK-removing this
    /// page. Absent/false on responses that returned everything.
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub acknowledged: Option<bool>,
    #[serde(default, deserialize_with = "bounded::deserialize_optional_code")]
    pub code: Option<String>,
    #[serde(default, deserialize_with = "bounded::deserialize_optional_payload")]
    pub payload: Option<String>,
    #[serde(default, deserialize_with = "bounded::deserialize_optional_response")]
    pub response: Option<String>,
    /// Number of blobs deleted by a purge request.
    #[serde(default)]
    pub blobs_deleted: Option<usize>,
    /// Recovery proofs returned by a query.
    #[serde(default, deserialize_with = "bounded::deserialize_proofs")]
    pub proofs: Option<Vec<V2RecoveryProof>>,
    /// Guardian entries returned by a query.
    #[serde(default, deserialize_with = "bounded::deserialize_guardians")]
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
            truncated: false,
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

// =========================================================================
// Tests
// =========================================================================

// INLINE_TEST_REQUIRED: tests exercise the private deserialize_with helpers
// and bounded-visitor internals that are intentionally not exported from this
// module. Moving them to tests/ would require making the helpers pub or
// adding a test-only facade, which adds unnecessary public surface area.
#[cfg(test)]
mod tests;
