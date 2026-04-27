// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Roundtrip tests for the offline recovery helpers on `Vauchi`.
//!
//! Covers `vauchi-core/src/api/vauchi/recovery_offline.rs`:
//! `parse_recovery_claim_b64`, `create_recovery_claim_hex_b64`,
//! `create_voucher_from_claim_b64`. These are pure encode/decode +
//! sign helpers — exercised through the AppEngine intercept layer in
//! production, but with no direct Rust test coverage before now.

use base64::Engine;
use vauchi_core::Vauchi;
use vauchi_core::VauchiError;
use vauchi_core::identity::Identity;
use vauchi_core::recovery::{RecoveryClaim, RecoveryVoucher};

fn vauchi_with_identity(name: &str) -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ============================================================
// create_recovery_claim_hex_b64
// ============================================================

// @internal
#[test]
fn create_recovery_claim_hex_b64_returns_decodable_claim() {
    let wb = vauchi_with_identity("Alice");
    let old_pk = [0xABu8; 32];
    let old_pk_hex = hex::encode(old_pk);

    let claim_b64 = wb.create_recovery_claim_hex_b64(&old_pk_hex).unwrap();

    // Round-trip the b64 → bytes → RecoveryClaim and confirm key bindings.
    let claim = wb.parse_recovery_claim_b64(&claim_b64).unwrap();
    assert_eq!(claim.old_pk(), &old_pk);
    assert_eq!(
        claim.new_pk(),
        wb.identity().unwrap().signing_public_key(),
        "new_pk must equal current identity's signing key"
    );
}

// @internal
#[test]
fn create_recovery_claim_hex_b64_trims_input() {
    let wb = vauchi_with_identity("Alice");
    let old_pk = [0x11u8; 32];
    let padded = format!("  {}\n", hex::encode(old_pk));

    let claim_b64 = wb.create_recovery_claim_hex_b64(&padded).unwrap();
    let claim = wb.parse_recovery_claim_b64(&claim_b64).unwrap();
    assert_eq!(claim.old_pk(), &old_pk);
}

// @internal
#[test]
fn create_recovery_claim_hex_b64_rejects_invalid_hex() {
    let wb = vauchi_with_identity("Alice");
    let result = wb.create_recovery_claim_hex_b64("not-hex");
    assert!(matches!(result, Err(VauchiError::Serialization(_))));
}

// @internal
#[test]
fn create_recovery_claim_hex_b64_rejects_wrong_length() {
    let wb = vauchi_with_identity("Alice");
    // 31 bytes hex (62 chars), not 32.
    let short = hex::encode([0u8; 31]);
    let result = wb.create_recovery_claim_hex_b64(&short);
    assert!(matches!(result, Err(VauchiError::Serialization(_))));
}

// @internal
#[test]
fn create_recovery_claim_hex_b64_requires_identity() {
    let wb = Vauchi::in_memory().unwrap();
    let result = wb.create_recovery_claim_hex_b64(&hex::encode([0u8; 32]));
    assert!(matches!(result, Err(VauchiError::IdentityNotInitialized)));
}

// ============================================================
// parse_recovery_claim_b64
// ============================================================

// @internal
#[test]
fn parse_recovery_claim_b64_decodes_valid_payload() {
    let wb = vauchi_with_identity("Alice");
    let claim_in = RecoveryClaim::new(&[0x01u8; 32], &[0x02u8; 32]);
    let payload = b64(&claim_in.to_bytes());

    let claim_out = wb.parse_recovery_claim_b64(&payload).unwrap();

    assert_eq!(claim_out.old_pk(), claim_in.old_pk());
    assert_eq!(claim_out.new_pk(), claim_in.new_pk());
}

// @internal
#[test]
fn parse_recovery_claim_b64_trims_whitespace() {
    let wb = vauchi_with_identity("Alice");
    let claim = RecoveryClaim::new(&[0x03u8; 32], &[0x04u8; 32]);
    let padded = format!("\n  {}\t", b64(&claim.to_bytes()));

    let parsed = wb.parse_recovery_claim_b64(&padded).unwrap();
    assert_eq!(parsed.old_pk(), claim.old_pk());
}

// @internal
#[test]
fn parse_recovery_claim_b64_rejects_invalid_base64() {
    let wb = vauchi_with_identity("Alice");
    let result = wb.parse_recovery_claim_b64("!!!not-base64!!!");
    assert!(matches!(result, Err(VauchiError::Serialization(_))));
}

// @internal
#[test]
fn parse_recovery_claim_b64_rejects_garbage_payload() {
    let wb = vauchi_with_identity("Alice");
    // Valid base64 of arbitrary bytes that don't deserialize as a claim.
    let result = wb.parse_recovery_claim_b64(&b64(&[0xFF; 8]));
    assert!(matches!(result, Err(VauchiError::Serialization(_))));
}

// ============================================================
// create_voucher_from_claim_b64
// ============================================================

// @internal
#[test]
fn create_voucher_from_claim_b64_signs_and_returns_decodable_voucher() {
    // Helper Bob signs a recovery claim issued by Alice's previous
    // identity (different signing key from Bob's) for Alice's new
    // identity (also different).
    let helper = vauchi_with_identity("Bob");

    let alice_old = Identity::create("Alice-Old");
    let alice_new = Identity::create("Alice-New");
    let claim = RecoveryClaim::new(
        alice_old.signing_public_key(),
        alice_new.signing_public_key(),
    );
    let claim_b64 = b64(&claim.to_bytes());

    let voucher_b64 = helper.create_voucher_from_claim_b64(&claim_b64).unwrap();

    let voucher_bytes = base64::engine::general_purpose::STANDARD
        .decode(&voucher_b64)
        .unwrap();
    let voucher = RecoveryVoucher::from_bytes(&voucher_bytes).unwrap();

    assert_eq!(voucher.old_pk(), alice_old.signing_public_key());
    assert_eq!(voucher.new_pk(), alice_new.signing_public_key());
    assert_eq!(
        voucher.voucher_pk(),
        helper.identity().unwrap().signing_public_key(),
        "voucher_pk must equal the helper's signing key"
    );
    assert!(
        voucher.verify(),
        "voucher signature must verify against its embedded voucher_pk"
    );
}

// @internal
#[test]
fn create_voucher_from_claim_b64_rejects_self_vouching() {
    // The helper IS the recovering identity → SelfVouching error.
    let alice = vauchi_with_identity("Alice");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let claim = RecoveryClaim::new(&[0xAAu8; 32], &alice_pk);
    let claim_b64 = b64(&claim.to_bytes());

    let result = alice.create_voucher_from_claim_b64(&claim_b64);
    assert!(matches!(result, Err(VauchiError::InvalidState(_))));
}

// @internal
#[test]
fn create_voucher_from_claim_b64_rejects_invalid_claim_payload() {
    let wb = vauchi_with_identity("Alice");
    let result = wb.create_voucher_from_claim_b64("!!!");
    assert!(matches!(result, Err(VauchiError::Serialization(_))));
}

// @internal
#[test]
fn create_voucher_from_claim_b64_requires_identity() {
    let wb = Vauchi::in_memory().unwrap();
    let claim = RecoveryClaim::new(&[1u8; 32], &[2u8; 32]);
    let result = wb.create_voucher_from_claim_b64(&b64(&claim.to_bytes()));
    assert!(matches!(result, Err(VauchiError::IdentityNotInitialized)));
}
