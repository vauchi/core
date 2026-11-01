// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
fn v2_send_request_accepts_origin_hint() {
    let json = r#"{"recipient_id":"aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899","ciphertext":"SGVsbG8=","origin_hint":"bm9uY2VfcGx1c19jaXBoZXJ0ZXh0"}"#;
    let req: V2SendRequest = serde_json::from_str(json).expect("valid send request with hint");
    assert_eq!(
        req.origin_hint.as_deref(),
        Some("bm9uY2VfcGx1c19jaXBoZXJ0ZXh0")
    );
}

// @internal
#[test]
fn v2_send_request_defaults_origin_hint_to_none() {
    let json = r#"{"recipient_id":"aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899","ciphertext":"SGVsbG8="}"#;
    let req: V2SendRequest = serde_json::from_str(json).expect("valid legacy send request");
    assert_eq!(req.origin_hint, None);
}

// @internal
#[test]
fn v2_send_request_rejects_oversized_origin_hint() {
    let json = format!(
        "{{\"recipient_id\":\"{}\",\"ciphertext\":\"SGVsbG8=\",\"origin_hint\":\"{}\"}}",
        "a".repeat(64),
        "A".repeat(MAX_ORIGIN_HINT_B64_LEN + 1)
    );
    let err = serde_json::from_str::<V2SendRequest>(&json).unwrap_err();
    assert!(err.to_string().contains("too long"));
}

// @internal
#[test]
fn v2_send_request_omits_origin_hint_when_none() {
    let req = V2SendRequest {
        recipient_id: "a".repeat(64),
        ciphertext: "SGVsbG8=".to_string(),
        origin_hint: None,
    };
    let json = serde_json::to_string(&req).expect("serialize send request");
    assert!(
        !json.contains("origin_hint"),
        "a None hint must not emit the field: {json}"
    );
}

// @internal
#[test]
fn fetched_blob_roundtrips_origin_hint() {
    let blob = FetchedBlob {
        blob_id: "b".repeat(64),
        ciphertext: "SGVsbG8=".to_string(),
        created_at: 42,
        mailbox_token: None,
        origin_hint: Some("aGludA==".to_string()),
    };
    let json = serde_json::to_string(&blob).expect("serialize blob");
    let back: FetchedBlob = serde_json::from_str(&json).expect("deserialize blob");
    assert_eq!(back.origin_hint.as_deref(), Some("aGludA=="));
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
