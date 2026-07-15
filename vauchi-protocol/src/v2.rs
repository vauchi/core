// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared V2 HTTP API request/response types.
//!
//! These types are the wire format between `vauchi-core` (client) and
//! `vauchi-relay` (server) for the V2 REST API. Both sides serialize and
//! deserialize with `serde_json`, so every type derives both `Serialize`
//! and `Deserialize`.
//!
//! Deserialization applies size limits as a defence-in-depth layer: the
//! relay performs its own validation, but rejecting oversized input during
//! parsing prevents large allocations and keeps the error path consistent
//! for malformed clients.

use serde::de::{self, Deserializer, Error as DeError, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::marker::PhantomData;

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

/// Maximum generic string length (1 MiB; generous upper bound for status/error text).
const MAX_STRING_LEN: usize = 1024 * 1024;

/// Maximum number of blobs in a V2 response
/// (slightly above the mailbox-token operational limit).
const MAX_BLOBS_LEN: usize = 2000;

/// Maximum number of recovery proofs in a query response.
const MAX_RECOVERY_PROOFS_LEN: usize = 50;

/// Maximum number of guardian entries in a query response.
const MAX_GUARDIANS_LEN: usize = 10;

// =========================================================================
// Bounded deserialization helpers
// =========================================================================

fn deserialize_bounded_string<'de, D>(deserializer: D, max_len: usize) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct BoundedStringVisitor(usize);

    impl<'de> Visitor<'de> for BoundedStringVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "a string of at most {} bytes", self.0)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if v.len() > self.0 {
                return Err(E::custom(format!(
                    "string too long: {} bytes (max {})",
                    v.len(),
                    self.0
                )));
            }
            Ok(v.to_owned())
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if v.len() > self.0 {
                return Err(E::custom(format!(
                    "string too long: {} bytes (max {})",
                    v.len(),
                    self.0
                )));
            }
            Ok(v)
        }
    }

    deserializer.deserialize_str(BoundedStringVisitor(max_len))
}

fn deserialize_bounded_vec_string<'de, D>(
    deserializer: D,
    max_items: usize,
    max_len: usize,
) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct BoundedVecStringVisitor {
        max_items: usize,
        max_len: usize,
    }

    impl<'de> Visitor<'de> for BoundedVecStringVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(
                formatter,
                "a list of at most {} strings, each at most {} bytes",
                self.max_items, self.max_len
            )
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(self.max_items));
            while let Some(value) = seq.next_element::<String>()? {
                if value.len() > self.max_len {
                    return Err(A::Error::custom(format!(
                        "string too long: {} bytes (max {})",
                        value.len(),
                        self.max_len
                    )));
                }
                if vec.len() >= self.max_items {
                    return Err(A::Error::custom(format!(
                        "too many items (max {})",
                        self.max_items
                    )));
                }
                vec.push(value);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_seq(BoundedVecStringVisitor { max_items, max_len })
}

fn deserialize_bounded_vec<'de, D, T>(deserializer: D, max_items: usize) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct BoundedVecVisitor<T> {
        max_items: usize,
        marker: PhantomData<T>,
    }

    impl<'de, T> Visitor<'de> for BoundedVecVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "a list of at most {} items", self.max_items)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(self.max_items));
            while let Some(value) = seq.next_element::<T>()? {
                if vec.len() >= self.max_items {
                    return Err(A::Error::custom(format!(
                        "too many items (max {})",
                        self.max_items
                    )));
                }
                vec.push(value);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_seq(BoundedVecVisitor {
        max_items,
        marker: PhantomData,
    })
}

fn deserialize_option_bounded_vec<'de, D, T>(
    deserializer: D,
    max_items: usize,
) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct OptionBoundedVecVisitor<T> {
        max_items: usize,
        marker: PhantomData<T>,
    }

    impl<'de, T> Visitor<'de> for OptionBoundedVecVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Option<Vec<T>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(
                formatter,
                "null or a list of at most {} items",
                self.max_items
            )
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_bounded_vec(deserializer, self.max_items).map(Some)
        }
    }

    deserializer.deserialize_option(OptionBoundedVecVisitor {
        max_items,
        marker: PhantomData,
    })
}

// Per-field wrappers (required by serde's `deserialize_with` attribute).

fn deserialize_recipient_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

fn deserialize_blob_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

fn deserialize_ciphertext<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

fn deserialize_mailbox_tokens<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec_string(deserializer, MAX_MAILBOX_TOKENS, MAX_ID_HEX_LEN)
}

fn deserialize_hex_hash<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

fn deserialize_public_key<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

fn deserialize_purge_token<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

fn deserialize_signature<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_SIGNATURE_HEX_LEN)
}

fn deserialize_exchange_payload<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

fn deserialize_exchange_code<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_STRING_LEN)
}

fn deserialize_exchange_response<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

fn deserialize_proof_data<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_PROOF_DATA_B64_LEN)
}

fn deserialize_key_hashes<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec_string(deserializer, MAX_RECOVERY_QUERY_HASHES, MAX_ID_HEX_LEN)
}

fn deserialize_guardian_entries<'de, D>(deserializer: D) -> Result<Vec<V2GuardianEntry>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec(deserializer, MAX_GUARDIAN_ENTRIES)
}

fn deserialize_guardian_entry_data<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_GUARDIAN_ENTRY_B64_LEN)
}

fn deserialize_designator_pk<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

fn deserialize_status<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_STRING_LEN)
}

fn deserialize_optional_bounded_string<'de, D>(
    deserializer: D,
    max_len: usize,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionalBoundedStringVisitor(usize);

    impl<'de> Visitor<'de> for OptionalBoundedStringVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "null or a string of at most {} bytes", self.0)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_bounded_string(deserializer, self.0).map(Some)
        }
    }

    deserializer.deserialize_option(OptionalBoundedStringVisitor(max_len))
}

fn deserialize_optional_error<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_STRING_LEN)
}

fn deserialize_optional_blob_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

fn deserialize_optional_code<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_STRING_LEN)
}

fn deserialize_optional_payload<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

fn deserialize_optional_response<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

fn deserialize_optional_mailbox_token<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

fn deserialize_blobs<'de, D>(deserializer: D) -> Result<Option<Vec<FetchedBlob>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_option_bounded_vec(deserializer, MAX_BLOBS_LEN)
}

fn deserialize_proofs<'de, D>(deserializer: D) -> Result<Option<Vec<V2RecoveryProof>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_option_bounded_vec(deserializer, MAX_RECOVERY_PROOFS_LEN)
}

fn deserialize_guardians<'de, D>(deserializer: D) -> Result<Option<Vec<V2GuardianEntry>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_option_bounded_vec(deserializer, MAX_GUARDIANS_LEN)
}

// =========================================================================
// Request / response types
// =========================================================================

/// V2 send request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2SendRequest {
    #[serde(deserialize_with = "deserialize_recipient_id")]
    pub recipient_id: String,
    /// Base64-encoded ciphertext.
    #[serde(deserialize_with = "deserialize_ciphertext")]
    pub ciphertext: String,
}

/// V2 fetch request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2FetchRequest {
    #[serde(deserialize_with = "deserialize_mailbox_tokens")]
    pub mailbox_tokens: Vec<String>,
}

/// V2 acknowledge request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2AckRequest {
    #[serde(deserialize_with = "deserialize_recipient_id")]
    pub recipient_id: String,
    #[serde(deserialize_with = "deserialize_blob_id")]
    pub blob_id: String,
}

/// V2 register mailbox tokens request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RegisterRequest {
    #[serde(deserialize_with = "deserialize_mailbox_tokens")]
    pub mailbox_tokens: Vec<String>,
}

/// V2 purge request body.
///
/// Purge is destructive — requires Ed25519 signature over
/// `public_key || purge_token || timestamp`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2PurgeRequest {
    #[serde(deserialize_with = "deserialize_recipient_id")]
    pub recipient_id: String,
    /// Hex-encoded Ed25519 public key (32 bytes).
    #[serde(deserialize_with = "deserialize_public_key")]
    pub public_key: String,
    /// Hex-encoded purge token (32 bytes).
    #[serde(deserialize_with = "deserialize_purge_token")]
    pub purge_token: String,
    /// Hex-encoded Ed25519 signature (64 bytes).
    #[serde(deserialize_with = "deserialize_signature")]
    pub signature: String,
    /// Unix timestamp (must be within 60s of server time).
    pub timestamp: u64,
}

/// V2 exchange offer request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeOfferRequest {
    #[serde(deserialize_with = "deserialize_exchange_payload")]
    pub payload: String,
    pub expires_secs: Option<u64>,
}

/// V2 exchange claim request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeClaimRequest {
    #[serde(deserialize_with = "deserialize_exchange_code")]
    pub code: String,
    #[serde(deserialize_with = "deserialize_exchange_response")]
    pub response: String,
}

/// V2 exchange complete request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ExchangeCompleteRequest {
    #[serde(deserialize_with = "deserialize_exchange_code")]
    pub code: String,
}

/// A fetched blob from the relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchedBlob {
    #[serde(deserialize_with = "deserialize_blob_id")]
    pub blob_id: String,
    /// Base64-encoded ciphertext.
    #[serde(deserialize_with = "deserialize_ciphertext")]
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
        deserialize_with = "deserialize_optional_mailbox_token"
    )]
    pub mailbox_token: Option<String>,
}

/// V2 recovery proof store request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryStoreRequest {
    /// Hex-encoded hash of the old public key (32 bytes = 64 hex chars).
    #[serde(deserialize_with = "deserialize_hex_hash")]
    pub key_hash: String,
    /// Base64-encoded recovery proof data (opaque to relay, max 4 KiB).
    #[serde(deserialize_with = "deserialize_proof_data")]
    pub proof_data: String,
}

/// V2 recovery proof query request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryQueryRequest {
    /// Hex-encoded key hashes to look up (max 50).
    #[serde(deserialize_with = "deserialize_key_hashes")]
    pub key_hashes: Vec<String>,
}

/// A single recovery proof entry in a query response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2RecoveryProof {
    #[serde(deserialize_with = "deserialize_hex_hash")]
    pub key_hash: String,
    /// Base64-encoded proof data.
    #[serde(deserialize_with = "deserialize_proof_data")]
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
    #[serde(deserialize_with = "deserialize_guardian_entry_data")]
    pub data: String,
}

/// V2 guardian store request body.
///
/// Atomically replaces all guardian entries for a given hash.
/// Removing a guardian = re-upload without their entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianStoreRequest {
    /// Hex-encoded hash of `designator_pk || "guardians"` (64 hex chars = 32 bytes).
    #[serde(deserialize_with = "deserialize_hex_hash")]
    pub guardian_hash: String,
    /// Encrypted entries (one per guardian, max 10).
    #[serde(deserialize_with = "deserialize_guardian_entries")]
    pub entries: Vec<V2GuardianEntry>,
    /// Hex-encoded Ed25519 designator public key (64 hex chars). The relay
    /// requires `SHA-256(designator_pk || "guardians") == guardian_hash`,
    /// proving the caller owns the identity the hash derives from.
    #[serde(deserialize_with = "deserialize_designator_pk")]
    pub designator_pk: String,
    /// Unix seconds; the relay rejects requests outside ±60s.
    pub timestamp: u64,
    /// Hex-encoded Ed25519 signature (128 hex chars) over
    /// `domain || designator_pk || guardian_hash || timestamp_be`.
    #[serde(deserialize_with = "deserialize_signature")]
    pub signature: String,
}

/// V2 guardian query request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianQueryRequest {
    /// Hex-encoded guardian hash to look up (64 hex chars = 32 bytes).
    #[serde(deserialize_with = "deserialize_hex_hash")]
    pub guardian_hash: String,
}

/// V2 guardian delete request body.
///
/// Deletes all guardian entries for a hash (identity purge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2GuardianDeleteRequest {
    /// Hex-encoded guardian hash to delete (64 hex chars = 32 bytes).
    #[serde(deserialize_with = "deserialize_hex_hash")]
    pub guardian_hash: String,
    /// Hex-encoded Ed25519 designator public key (64 hex chars). The relay
    /// requires `SHA-256(designator_pk || "guardians") == guardian_hash`,
    /// proving the caller owns the identity the hash derives from.
    #[serde(deserialize_with = "deserialize_designator_pk")]
    pub designator_pk: String,
    /// Unix seconds; the relay rejects requests outside ±60s.
    pub timestamp: u64,
    /// Hex-encoded Ed25519 signature (128 hex chars) over
    /// `domain || designator_pk || guardian_hash || timestamp_be`.
    #[serde(deserialize_with = "deserialize_signature")]
    pub signature: String,
}

/// Standard V2 response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct V2Response {
    #[serde(deserialize_with = "deserialize_status")]
    pub status: String,
    #[serde(default, deserialize_with = "deserialize_optional_error")]
    pub error: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_blob_id")]
    pub blob_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_blobs")]
    pub blobs: Option<Vec<FetchedBlob>>,
    #[serde(default)]
    pub acknowledged: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_code")]
    pub code: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_payload")]
    pub payload: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_response")]
    pub response: Option<String>,
    /// Number of blobs deleted by a purge request.
    #[serde(default)]
    pub blobs_deleted: Option<usize>,
    /// Recovery proofs returned by a query.
    #[serde(default, deserialize_with = "deserialize_proofs")]
    pub proofs: Option<Vec<V2RecoveryProof>>,
    /// Guardian entries returned by a query.
    #[serde(default, deserialize_with = "deserialize_guardians")]
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

// =========================================================================
// Tests
// =========================================================================

// INLINE_TEST_REQUIRED: tests exercise the private deserialize_with helpers
// and bounded-visitor internals that are intentionally not exported from this
// module. Moving them to tests/ would require making the helpers pub or
// adding a test-only facade, which adds unnecessary public surface area.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn v2_send_request_accepts_valid_input() {
        let json = r#"{"recipient_id":"aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899","ciphertext":"SGVsbG8="}"#;
        let req: V2SendRequest = serde_json::from_str(json).expect("valid send request");
        assert_eq!(req.recipient_id.len(), 64);
        assert_eq!(req.ciphertext, "SGVsbG8=");
    }

    // @internal
    #[test]
    fn v2_send_request_rejects_oversized_ciphertext() {
        let json = format!(
            "{{\"recipient_id\":\"{}\",\"ciphertext\":\"{}\"}}",
            "a".repeat(64),
            "A".repeat(MAX_CIPHERTEXT_B64_LEN + 1)
        );
        let err = serde_json::from_str::<V2SendRequest>(&json).unwrap_err();
        assert!(err.to_string().contains("too long"));
    }

    // @internal
    #[test]
    fn v2_fetch_request_rejects_too_many_tokens() {
        let tokens: Vec<String> = (0..MAX_MAILBOX_TOKENS + 1)
            .map(|i| format!("{:064x}", i))
            .collect();
        let json = format!(
            "{{\"mailbox_tokens\":{}}}",
            serde_json::to_string(&tokens).unwrap()
        );
        let err = serde_json::from_str::<V2FetchRequest>(&json).unwrap_err();
        assert!(err.to_string().contains("too many items"));
    }

    // @internal
    #[test]
    fn v2_fetch_request_rejects_oversized_token() {
        let json = format!(
            "{{\"mailbox_tokens\":[\"{}\"]}}",
            "a".repeat(MAX_ID_HEX_LEN + 1)
        );
        let err = serde_json::from_str::<V2FetchRequest>(&json).unwrap_err();
        assert!(err.to_string().contains("too long"));
    }

    // @internal
    #[test]
    fn v2_recovery_store_request_limits_proof_data() {
        let json = format!(
            "{{\"key_hash\":\"{}\",\"proof_data\":\"{}\"}}",
            "a".repeat(64),
            "A".repeat(MAX_PROOF_DATA_B64_LEN + 1)
        );
        let err = serde_json::from_str::<V2RecoveryStoreRequest>(&json).unwrap_err();
        assert!(err.to_string().contains("too long"));
    }

    // @internal
    #[test]
    fn v2_recovery_query_request_limits_key_hashes() {
        let hashes: Vec<String> = (0..MAX_RECOVERY_QUERY_HASHES + 1)
            .map(|i| format!("{:064x}", i))
            .collect();
        let json = format!(
            "{{\"key_hashes\":{}}}",
            serde_json::to_string(&hashes).unwrap()
        );
        let err = serde_json::from_str::<V2RecoveryQueryRequest>(&json).unwrap_err();
        assert!(err.to_string().contains("too many items"));
    }

    // @internal
    #[test]
    fn v2_guardian_store_request_limits_entries() {
        let entries: Vec<V2GuardianEntry> = (0..MAX_GUARDIAN_ENTRIES + 1)
            .map(|_| V2GuardianEntry {
                data: "SGVsbG8=".to_string(),
            })
            .collect();
        let req = V2GuardianStoreRequest {
            guardian_hash: "a".repeat(64),
            entries,
            designator_pk: "b".repeat(64),
            timestamp: 0,
            signature: "c".repeat(128),
        };
        let json = serde_json::to_string(&req).unwrap();
        let err = serde_json::from_str::<V2GuardianStoreRequest>(&json).unwrap_err();
        assert!(err.to_string().contains("too many items"));
    }

    // @internal
    #[test]
    fn v2_guardian_entry_limits_data() {
        let json = format!(
            "{{\"data\":\"{}\"}}",
            "A".repeat(MAX_GUARDIAN_ENTRY_B64_LEN + 1)
        );
        let err = serde_json::from_str::<V2GuardianEntry>(&json).unwrap_err();
        assert!(err.to_string().contains("too long"));
    }

    // @internal
    #[test]
    fn v2_response_limits_blobs() {
        let json = format!(
            "{{\"status\":\"ok\",\"blobs\":{}}}",
            serde_json::to_string(&vec![
                FetchedBlob {
                    blob_id: "a".repeat(64),
                    ciphertext: "SGVsbG8=".to_string(),
                    created_at: 0,
                    mailbox_token: None,
                };
                MAX_BLOBS_LEN + 1
            ])
            .unwrap()
        );
        let err = serde_json::from_str::<V2Response>(&json).unwrap_err();
        assert!(err.to_string().contains("too many items"));
    }

    // @internal
    #[test]
    fn v2_response_accepts_null_vectors() {
        // Legacy relays/clients may explicitly send null for optional vectors.
        let json = r#"{"status":"ok","blobs":null,"proofs":null,"guardians":null}"#;
        let resp: V2Response = serde_json::from_str(json).expect("null vectors accepted");
        assert!(resp.blobs.is_none());
        assert!(resp.proofs.is_none());
        assert!(resp.guardians.is_none());
    }

    // @internal
    #[test]
    fn v2_response_limits_optional_strings() {
        let json = format!(
            "{{\"status\":\"ok\",\"error\":\"{}\"}}",
            "x".repeat(MAX_STRING_LEN + 1)
        );
        let err = serde_json::from_str::<V2Response>(&json).unwrap_err();
        assert!(err.to_string().contains("too long"));
    }
}
