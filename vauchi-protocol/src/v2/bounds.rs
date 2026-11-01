// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The bounds the V2 deserializers enforce, named per type and field so the
//! published wire schema can carry them and a test can prove each one
//! against the parser (`tests/it/wire_schema_tests.rs`, private#119). The
//! visitors in `bounded.rs` stay the authority; this table says which
//! constant each field reads, and the test fails when the two disagree.

use super::*;

/// One field's parse-boundary bound. Lengths are in bytes, as the
/// visitors count them; every bounded field is ASCII (hex or base64),
/// so a schema's character count means the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldBound {
    pub type_name: &'static str,
    pub field: &'static str,
    /// Most bytes a string field accepts.
    pub max_len: Option<usize>,
    /// Most items a list field accepts.
    pub max_items: Option<usize>,
    /// Most bytes each string item of a list accepts.
    pub item_max_len: Option<usize>,
}

const fn string(type_name: &'static str, field: &'static str, max_len: usize) -> FieldBound {
    FieldBound {
        type_name,
        field,
        max_len: Some(max_len),
        max_items: None,
        item_max_len: None,
    }
}

const fn strings(
    type_name: &'static str,
    field: &'static str,
    max_items: usize,
    item_max_len: usize,
) -> FieldBound {
    FieldBound {
        type_name,
        field,
        max_len: None,
        max_items: Some(max_items),
        item_max_len: Some(item_max_len),
    }
}

const fn items(type_name: &'static str, field: &'static str, max_items: usize) -> FieldBound {
    FieldBound {
        type_name,
        field,
        max_len: None,
        max_items: Some(max_items),
        item_max_len: None,
    }
}

/// Every bound a V2 type enforces while parsing, by type and field.
pub const FIELD_BOUNDS: &[FieldBound] = &[
    string("V2SendRequest", "recipient_id", MAX_ID_HEX_LEN),
    string("V2SendRequest", "ciphertext", MAX_CIPHERTEXT_B64_LEN),
    string("V2SendRequest", "origin_hint", MAX_ORIGIN_HINT_B64_LEN),
    strings(
        "V2FetchRequest",
        "mailbox_tokens",
        MAX_MAILBOX_TOKENS,
        MAX_ID_HEX_LEN,
    ),
    string("V2AckRequest", "recipient_id", MAX_ID_HEX_LEN),
    string("V2AckRequest", "blob_id", MAX_ID_HEX_LEN),
    strings(
        "V2RegisterRequest",
        "mailbox_tokens",
        MAX_MAILBOX_TOKENS,
        MAX_ID_HEX_LEN,
    ),
    string("V2PurgeRequest", "public_key", MAX_ID_HEX_LEN),
    string("V2PurgeRequest", "purge_token", MAX_ID_HEX_LEN),
    string("V2PurgeRequest", "signature", MAX_SIGNATURE_HEX_LEN),
    string("V2ExchangeOfferRequest", "payload", MAX_CIPHERTEXT_B64_LEN),
    string("V2ExchangeClaimRequest", "code", MAX_STRING_LEN),
    string("V2ExchangeClaimRequest", "response", MAX_CIPHERTEXT_B64_LEN),
    string("V2ExchangeCompleteRequest", "code", MAX_STRING_LEN),
    string("FetchedBlob", "blob_id", MAX_ID_HEX_LEN),
    string("FetchedBlob", "ciphertext", MAX_CIPHERTEXT_B64_LEN),
    string("FetchedBlob", "mailbox_token", MAX_ID_HEX_LEN),
    string("FetchedBlob", "origin_hint", MAX_ORIGIN_HINT_B64_LEN),
    string("V2RecoveryStoreRequest", "key_hash", MAX_ID_HEX_LEN),
    string(
        "V2RecoveryStoreRequest",
        "proof_data",
        MAX_PROOF_DATA_B64_LEN,
    ),
    string("V2RecoveryStoreRequest", "owner_pk", MAX_ID_HEX_LEN),
    string("V2RecoveryStoreRequest", "signature", MAX_SIGNATURE_HEX_LEN),
    strings(
        "V2RecoveryQueryRequest",
        "key_hashes",
        MAX_RECOVERY_QUERY_HASHES,
        MAX_ID_HEX_LEN,
    ),
    string("V2RecoveryProof", "key_hash", MAX_ID_HEX_LEN),
    string("V2RecoveryProof", "proof_data", MAX_PROOF_DATA_B64_LEN),
    string("V2GuardianEntry", "data", MAX_GUARDIAN_ENTRY_B64_LEN),
    string("V2GuardianStoreRequest", "guardian_hash", MAX_ID_HEX_LEN),
    items("V2GuardianStoreRequest", "entries", MAX_GUARDIAN_ENTRIES),
    string("V2GuardianStoreRequest", "designator_pk", MAX_ID_HEX_LEN),
    string("V2GuardianStoreRequest", "signature", MAX_SIGNATURE_HEX_LEN),
    string("V2GuardianQueryRequest", "guardian_hash", MAX_ID_HEX_LEN),
    string("V2GuardianDeleteRequest", "guardian_hash", MAX_ID_HEX_LEN),
    string("V2GuardianDeleteRequest", "designator_pk", MAX_ID_HEX_LEN),
    string(
        "V2GuardianDeleteRequest",
        "signature",
        MAX_SIGNATURE_HEX_LEN,
    ),
    string("V2Response", "status", MAX_STRING_LEN),
    string("V2Response", "error", MAX_STRING_LEN),
    string("V2Response", "blob_id", MAX_ID_HEX_LEN),
    string("V2Response", "code", MAX_STRING_LEN),
    string("V2Response", "payload", MAX_CIPHERTEXT_B64_LEN),
    string("V2Response", "response", MAX_CIPHERTEXT_B64_LEN),
    items("V2Response", "proofs", MAX_RECOVERY_PROOFS_LEN),
    items("V2Response", "guardians", MAX_GUARDIANS_LEN),
];
