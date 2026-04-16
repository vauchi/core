// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unit tests for escrow protocol types.

use vauchi_protocol::escrow::*;

// A valid 64-char hex string (32 bytes).
fn valid_hash() -> String {
    "0123456789abcdef".repeat(4)
}

fn valid_hash_alt() -> String {
    "ff".repeat(32)
}

fn small_blob_b64() -> String {
    // base64 of "hello"
    "aGVsbG8=".to_string()
}

// ================================================================
// Serde roundtrip — EscrowMessage
// ================================================================

// @internal
#[test]
fn put_message_roundtrip() {
    let msg = EscrowMessage::Put {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
        blob: small_blob_b64(),
        ttl_seconds: 3600,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: EscrowMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

// @internal
#[test]
fn get_message_roundtrip() {
    let msg = EscrowMessage::Get {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: EscrowMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

// @internal
#[test]
fn count_message_roundtrip() {
    let msg = EscrowMessage::Count {
        gate_hash: valid_hash(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: EscrowMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

// ================================================================
// Serde roundtrip — EscrowResponse
// ================================================================

// @internal
#[test]
fn stored_response_roundtrip() {
    let resp = EscrowResponse::Stored;
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// @internal
#[test]
fn already_exists_response_roundtrip() {
    let resp = EscrowResponse::AlreadyExists;
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// @internal
#[test]
fn gate_full_response_roundtrip() {
    let resp = EscrowResponse::GateFull;
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// @internal
#[test]
fn blob_too_large_response_roundtrip() {
    let resp = EscrowResponse::BlobTooLarge;
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// @internal
#[test]
fn blob_response_roundtrip() {
    let resp = EscrowResponse::Blob {
        blob: small_blob_b64(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// @internal
#[test]
fn not_ready_response_roundtrip() {
    let resp = EscrowResponse::NotReady { count: 1 };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// @internal
#[test]
fn count_response_roundtrip() {
    let resp = EscrowResponse::Count { count: 2 };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// @internal
#[test]
fn not_found_response_roundtrip() {
    let resp = EscrowResponse::NotFound;
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// ================================================================
// JSON wire format — tagged discriminator
// ================================================================

// @internal
#[test]
fn put_message_has_action_tag() {
    let msg = EscrowMessage::Put {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
        blob: small_blob_b64(),
        ttl_seconds: 3600,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""action":"Put"#));
}

// @internal
#[test]
fn stored_response_has_status_tag() {
    let resp = EscrowResponse::Stored;
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains(r#""status":"Stored"#));
}

// @internal
#[test]
fn response_deserializes_from_minimal_json() {
    let json = r#"{"status":"NotFound"}"#;
    let parsed: EscrowResponse = serde_json::from_str(json).unwrap();
    assert_eq!(parsed, EscrowResponse::NotFound);
}

// ================================================================
// Validation — valid messages
// ================================================================

// @internal
#[test]
fn valid_put_passes_validation() {
    let msg = EscrowMessage::Put {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
        blob: small_blob_b64(),
        ttl_seconds: 3600,
    };
    assert!(msg.validate().is_ok());
}

// @internal
#[test]
fn valid_get_passes_validation() {
    let msg = EscrowMessage::Get {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
    };
    assert!(msg.validate().is_ok());
}

// @internal
#[test]
fn valid_count_passes_validation() {
    let msg = EscrowMessage::Count {
        gate_hash: valid_hash(),
    };
    assert!(msg.validate().is_ok());
}

// ================================================================
// Validation — invalid hashes
// ================================================================

// @internal
#[test]
fn put_rejects_short_gate_hash() {
    let msg = EscrowMessage::Put {
        gate_hash: "abcd".to_string(),
        slot_hash: valid_hash_alt(),
        blob: small_blob_b64(),
        ttl_seconds: 3600,
    };
    let errs = msg.validate().unwrap_err();
    assert!(errs.contains(&EscrowValidationError::InvalidGateHash));
}

// @internal
#[test]
fn put_rejects_non_hex_gate_hash() {
    let msg = EscrowMessage::Put {
        gate_hash: "zz".repeat(32),
        slot_hash: valid_hash_alt(),
        blob: small_blob_b64(),
        ttl_seconds: 3600,
    };
    let errs = msg.validate().unwrap_err();
    assert!(errs.contains(&EscrowValidationError::InvalidGateHash));
}

// @internal
#[test]
fn get_rejects_invalid_slot_hash() {
    let msg = EscrowMessage::Get {
        gate_hash: valid_hash(),
        slot_hash: "short".to_string(),
    };
    let errs = msg.validate().unwrap_err();
    assert!(errs.contains(&EscrowValidationError::InvalidSlotHash));
}

// @internal
#[test]
fn count_rejects_empty_gate_hash() {
    let msg = EscrowMessage::Count {
        gate_hash: String::new(),
    };
    let errs = msg.validate().unwrap_err();
    assert!(errs.contains(&EscrowValidationError::InvalidGateHash));
}

// ================================================================
// Validation — blob size
// ================================================================

// @internal
#[test]
fn put_rejects_oversized_blob() {
    // base64 of 65537 bytes > MAX_BLOB_BYTES
    let oversized = "A".repeat(87_384); // ceil(65537 * 4/3) ≈ 87383
    let msg = EscrowMessage::Put {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
        blob: oversized,
        ttl_seconds: 3600,
    };
    let errs = msg.validate().unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, EscrowValidationError::BlobTooLarge { .. }))
    );
}

// @internal
#[test]
fn put_accepts_max_size_blob() {
    // base64 of exactly MAX_BLOB_BYTES: ceil(65536 * 4/3) = 87382
    let max_b64 = "A".repeat(87_382);
    let msg = EscrowMessage::Put {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
        blob: max_b64,
        ttl_seconds: 3600,
    };
    // 87382 * 3 / 4 = 65536 (integer division) — exactly at limit.
    assert!(msg.validate().is_ok());
}

// ================================================================
// Validation — TTL
// ================================================================

// @internal
#[test]
fn put_rejects_excessive_ttl() {
    let msg = EscrowMessage::Put {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
        blob: small_blob_b64(),
        ttl_seconds: MAX_TTL_SECONDS + 1,
    };
    let errs = msg.validate().unwrap_err();
    assert!(errs.contains(&EscrowValidationError::TtlTooLong {
        ttl: MAX_TTL_SECONDS + 1
    }));
}

// @internal
#[test]
fn put_accepts_max_ttl() {
    let msg = EscrowMessage::Put {
        gate_hash: valid_hash(),
        slot_hash: valid_hash_alt(),
        blob: small_blob_b64(),
        ttl_seconds: MAX_TTL_SECONDS,
    };
    assert!(msg.validate().is_ok());
}

// ================================================================
// Validation — multiple errors
// ================================================================

// @internal
#[test]
fn put_reports_all_errors_at_once() {
    let msg = EscrowMessage::Put {
        gate_hash: "bad".to_string(),
        slot_hash: "also_bad".to_string(),
        blob: "A".repeat(100_000),
        ttl_seconds: MAX_TTL_SECONDS + 1,
    };
    let errs = msg.validate().unwrap_err();
    assert_eq!(errs.len(), 4, "all 4 validation errors should be reported");
}

// ================================================================
// Constants
// ================================================================

// @internal
#[test]
fn max_blob_bytes_is_64_kib() {
    assert_eq!(MAX_BLOB_BYTES, 65_536);
}

// @internal
#[test]
fn max_ttl_is_7_days() {
    assert_eq!(MAX_TTL_SECONDS, 7 * 24 * 60 * 60);
}

// @internal
#[test]
fn max_slots_per_gate_is_2() {
    assert_eq!(MAX_SLOTS_PER_GATE, 2);
}

// @internal
#[test]
fn hash_hex_length_is_64() {
    assert_eq!(HASH_HEX_LENGTH, 64);
}
