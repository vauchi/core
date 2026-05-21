// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the public surface of `vauchi_core::identifiers`.
//!
//! Covers the `IdentityKey` newtype contract that the recovery
//! struct swap-overs (Phase 1A of
//! `_private/docs/problems/2026-05-21-wire-identifier-newtypes/`)
//! depend on:
//! - `from_bytes` / `as_bytes` / `into_bytes` round-trip
//! - `PartialEq` / `Hash` agree with the underlying bytes (matters
//!   for `HashSet<IdentityKey>` uses in `RecoveryProof`)
//! - `Display` produces lowercase hex (used as the diagnostic /
//!   UI rendering for keys via `to_string()`)
//! - `AsRef<[u8]>` keeps `hex::encode(claim.old_pk())` compiling
//!   unchanged at the 30+ downstream sites in vauchi-app and
//!   vauchi-platform
//! - `#[serde(transparent)]` preserves the on-disk JSON shape of
//!   persisted `RecoveryProgress` (byte-identical to the underlying
//!   `[u8; 32]`)

use std::collections::HashSet;
use vauchi_core::identifiers::{ContactId, DhPublicKey, IdentityKey};

// @internal
#[test]
fn roundtrip_through_from_and_as_bytes() {
    let bytes = [0xABu8; 32];
    let key = IdentityKey::from(bytes);
    assert_eq!(key.as_bytes(), &bytes);
    assert_eq!(key.into_bytes(), bytes);
}

// @internal
#[test]
fn equal_underlying_bytes_compare_equal() {
    let a = IdentityKey::from_bytes([1u8; 32]);
    let b = IdentityKey::from_bytes([1u8; 32]);
    let c = IdentityKey::from_bytes([2u8; 32]);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// @internal
#[test]
fn hash_matches_when_bytes_match() {
    let mut set = HashSet::new();
    set.insert(IdentityKey::from_bytes([7u8; 32]));
    assert!(set.contains(&IdentityKey::from_bytes([7u8; 32])));
    assert!(!set.contains(&IdentityKey::from_bytes([8u8; 32])));
}

// @internal
#[test]
fn display_format_is_lowercase_hex() {
    let key = IdentityKey::from_bytes([0xDE; 32]);
    let expected: String = std::iter::repeat("de").take(32).collect();
    assert_eq!(key.to_string(), expected);
}

// @internal
#[test]
fn hex_encode_compatibility_via_asref_bytes() {
    // Many call sites do `hex::encode(claim.old_pk())`. After the
    // recovery-struct swap-over, `old_pk()` returns `&IdentityKey`;
    // `hex::encode` must keep compiling and produce the same output
    // as the bare-bytes form.
    let bytes = [0x42u8; 32];
    let key = IdentityKey::from_bytes(bytes);
    assert_eq!(hex::encode(&key), hex::encode(bytes));
}

// @internal
#[test]
fn serde_transparent_matches_underlying_array_shape() {
    // Phase 1A wire-shape guarantee: serializing an `IdentityKey`
    // produces byte-identical JSON to serializing `[u8; 32]`. This
    // preserves the on-disk `RecoveryProgress` JSON across the
    // recovery-struct newtype migration (storage/recovery.rs:195
    // `serde_json::to_vec(progress)`).
    let bytes = [3u8; 32];
    let key = IdentityKey::from_bytes(bytes);
    let key_json = serde_json::to_string(&key).expect("serialize key");
    let bytes_json = serde_json::to_string(&bytes).expect("serialize bytes");
    assert_eq!(key_json, bytes_json);

    let deserialized: IdentityKey =
        serde_json::from_str(&bytes_json).expect("deserialize bytes-shape into key");
    assert_eq!(deserialized, key);
}

// Phase 1B: DhPublicKey mirrors IdentityKey's contract but is a
// nominally distinct type (X25519 DH key, not Ed25519). The tests
// below mirror IdentityKey's tests so the two stay in lockstep.

// @internal
#[test]
fn dh_pubkey_roundtrip_through_from_and_as_bytes() {
    let bytes = [0xCDu8; 32];
    let key = DhPublicKey::from(bytes);
    assert_eq!(key.as_bytes(), &bytes);
    assert_eq!(key.into_bytes(), bytes);
}

// @internal
#[test]
fn dh_pubkey_equal_underlying_bytes_compare_equal() {
    let a = DhPublicKey::from_bytes([1u8; 32]);
    let b = DhPublicKey::from_bytes([1u8; 32]);
    let c = DhPublicKey::from_bytes([2u8; 32]);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// @internal
#[test]
fn dh_pubkey_hash_matches_when_bytes_match() {
    let mut set = HashSet::new();
    set.insert(DhPublicKey::from_bytes([7u8; 32]));
    assert!(set.contains(&DhPublicKey::from_bytes([7u8; 32])));
    assert!(!set.contains(&DhPublicKey::from_bytes([8u8; 32])));
}

// @internal
#[test]
fn dh_pubkey_display_format_is_lowercase_hex() {
    let key = DhPublicKey::from_bytes([0xDE; 32]);
    let expected: String = std::iter::repeat("de").take(32).collect();
    assert_eq!(key.to_string(), expected);
}

// @internal
#[test]
fn dh_pubkey_serde_transparent_matches_underlying_array_shape() {
    let bytes = [3u8; 32];
    let key = DhPublicKey::from_bytes(bytes);
    let key_json = serde_json::to_string(&key).expect("serialize key");
    let bytes_json = serde_json::to_string(&bytes).expect("serialize bytes");
    assert_eq!(key_json, bytes_json);

    let deserialized: DhPublicKey =
        serde_json::from_str(&bytes_json).expect("deserialize bytes-shape into key");
    assert_eq!(deserialized, key);
}

// Wire serde adapters: confirm the per-field base64 adapters
// produce the same shape as the existing `bytes_array_32` modules
// so swapping a wire field's type does not break the JSON layout.

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
struct WireIdentityKeyHarness {
    #[serde(with = "vauchi_core::identifiers::wire_identity_key_base64")]
    key: IdentityKey,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
struct WireDhPublicKeyHarness {
    #[serde(with = "vauchi_core::identifiers::wire_dh_public_key_base64")]
    key: DhPublicKey,
}

// @internal
#[test]
fn wire_identity_key_adapter_emits_base64_string() {
    let bytes = [0x11u8; 32];
    let h = WireIdentityKeyHarness {
        key: IdentityKey::from_bytes(bytes),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&h).unwrap()).unwrap();
    let expected = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    assert_eq!(json["key"], serde_json::Value::String(expected));

    let restored: WireIdentityKeyHarness = serde_json::from_value(json).unwrap();
    assert_eq!(restored, h);
}

// @internal
#[test]
fn wire_dh_public_key_adapter_emits_base64_string() {
    let bytes = [0x22u8; 32];
    let h = WireDhPublicKeyHarness {
        key: DhPublicKey::from_bytes(bytes),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&h).unwrap()).unwrap();
    let expected = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    assert_eq!(json["key"], serde_json::Value::String(expected));

    let restored: WireDhPublicKeyHarness = serde_json::from_value(json).unwrap();
    assert_eq!(restored, h);
}

// @internal
#[test]
fn wire_identity_key_adapter_rejects_wrong_length() {
    let too_short = serde_json::json!({ "key": "AQID" }); // 3 bytes
    let result: Result<WireIdentityKeyHarness, _> = serde_json::from_value(too_short);
    assert!(
        result.is_err(),
        "wire adapter must reject base64 that does not decode to 32 bytes"
    );
}

// @internal
#[test]
fn wire_dh_public_key_adapter_rejects_wrong_length() {
    let too_short = serde_json::json!({ "key": "AQID" }); // 3 bytes
    let result: Result<WireDhPublicKeyHarness, _> = serde_json::from_value(too_short);
    assert!(
        result.is_err(),
        "wire adapter must reject base64 that does not decode to 32 bytes"
    );
}

// @internal
#[test]
fn wire_identity_key_adapter_rejects_invalid_base64_chars() {
    // The string is 44 chars (correct length for a base64-encoded
    // 32-byte value) but contains chars outside the base64 alphabet.
    // The adapter must surface the decode failure rather than silently
    // truncating or producing a malformed key.
    let malformed = serde_json::json!({ "key": "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!" });
    let result: Result<WireIdentityKeyHarness, _> = serde_json::from_value(malformed);
    assert!(
        result.is_err(),
        "wire adapter must reject non-base64 characters"
    );
}

// @internal
#[test]
fn wire_dh_public_key_adapter_rejects_invalid_base64_chars() {
    let malformed = serde_json::json!({ "key": "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!" });
    let result: Result<WireDhPublicKeyHarness, _> = serde_json::from_value(malformed);
    assert!(
        result.is_err(),
        "wire adapter must reject non-base64 characters"
    );
}

// @internal
#[test]
fn wire_identity_key_adapter_rejects_non_string_json() {
    // A JSON integer-array can deserialize transparently into an
    // IdentityKey via the type's default serde, but the wire adapter
    // is base64-only — it must reject any non-string JSON value at
    // the wire boundary.
    let array_shape = serde_json::json!({ "key": vec![0u8; 32] });
    let result: Result<WireIdentityKeyHarness, _> = serde_json::from_value(array_shape);
    assert!(
        result.is_err(),
        "wire adapter must reject non-string JSON values"
    );
}

// Phase 2: ContactId mirrors the IdentityKey / DhPublicKey contract
// but wraps a `String` (hex-fingerprint or UUID wire identifier on
// `sender_id` / `recipient_id`). Tests below mirror the existing
// newtype tests so the three stay in lockstep.

const CONTACT_ID_HEX: &str = "deadbeefcafef00d0123456789abcdef0011223344556677889900aabbccddee";

// @internal
#[test]
fn contact_id_roundtrip_through_from_and_into_string() {
    let id = ContactId::from(CONTACT_ID_HEX.to_string());
    assert_eq!(id.as_str(), CONTACT_ID_HEX);
    assert_eq!(id.clone().into_string(), CONTACT_ID_HEX);
}

// @internal
#[test]
fn contact_id_from_str_matches_from_string() {
    let from_str = ContactId::from(CONTACT_ID_HEX);
    let from_string = ContactId::from(CONTACT_ID_HEX.to_string());
    assert_eq!(from_str, from_string);
}

// @internal
#[test]
fn contact_id_equal_underlying_strings_compare_equal() {
    let a = ContactId::from_string(CONTACT_ID_HEX.to_string());
    let b = ContactId::from_string(CONTACT_ID_HEX.to_string());
    let c = ContactId::from_string(
        "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    );
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// @internal
#[test]
fn contact_id_hash_matches_when_strings_match() {
    let a = ContactId::from_string(CONTACT_ID_HEX.to_string());
    let b = ContactId::from_string(CONTACT_ID_HEX.to_string());
    let mut set: HashSet<ContactId> = HashSet::new();
    set.insert(a);
    assert!(set.contains(&b));
}

// @internal
#[test]
fn contact_id_display_is_the_underlying_string() {
    let id = ContactId::from(CONTACT_ID_HEX);
    assert_eq!(format!("{id}"), CONTACT_ID_HEX);
}

// @internal
#[test]
fn contact_id_forward_partial_eq_with_str_works_on_str_literal() {
    let id = ContactId::from(CONTACT_ID_HEX);
    assert!(id == CONTACT_ID_HEX);
}

// @internal
#[test]
fn contact_id_as_ref_str_and_bytes_borrow_the_underlying_value() {
    let id = ContactId::from(CONTACT_ID_HEX);
    let s: &str = id.as_ref();
    assert_eq!(s, CONTACT_ID_HEX);
    let b: &[u8] = id.as_ref();
    assert_eq!(b, CONTACT_ID_HEX.as_bytes());
}

// @internal
#[test]
fn contact_id_serde_transparent_matches_raw_string_shape() {
    let id = ContactId::from(CONTACT_ID_HEX);
    let json = serde_json::to_value(&id).unwrap();
    assert_eq!(json, serde_json::Value::String(CONTACT_ID_HEX.to_string()));
    let from_raw_string: ContactId =
        serde_json::from_value(serde_json::Value::String(CONTACT_ID_HEX.to_string())).unwrap();
    assert_eq!(from_raw_string, id);
}
