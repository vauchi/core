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
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::identity::Identity;
use vauchi_core::recovery::{RecoveryClaim, RecoveryVoucher};
use vauchi_core::{SymmetricKey, clock::SystemClock};

fn vauchi_with_identity(name: &str) -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn now() -> u64 {
    SystemClock::shared().unix_seconds()
}

/// Adds `identity` to `helper`'s contacts under `name`, as an in-person
/// exchange would.
fn add_contact(helper: &Vauchi, identity: &Identity, name: &str) {
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    helper.add_contact(contact).unwrap();
}

fn claim_b64(old: &Identity, new: &Identity, timestamp: u64) -> String {
    let claim = RecoveryClaim::new(
        old.signing_public_key(),
        new.signing_public_key(),
        timestamp,
    );
    b64(&claim.to_bytes())
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
fn create_recovery_claim_hex_b64_stamps_the_current_time() {
    let wb = vauchi_with_identity("Alice");
    let before = now();

    let claim_b64 = wb
        .create_recovery_claim_hex_b64(&hex::encode([0xABu8; 32]))
        .unwrap();

    let claim = wb.parse_recovery_claim_b64(&claim_b64).unwrap();
    assert!((before..=now()).contains(&claim.timestamp()));
    assert!(!claim.is_expired(now()));
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
    let claim_in = RecoveryClaim::new(&[0x01u8; 32], &[0x02u8; 32], 0);
    let payload = b64(&claim_in.to_bytes());

    let claim_out = wb.parse_recovery_claim_b64(&payload).unwrap();

    assert_eq!(claim_out.old_pk(), claim_in.old_pk());
    assert_eq!(claim_out.new_pk(), claim_in.new_pk());
}

// @internal
#[test]
fn parse_recovery_claim_b64_trims_whitespace() {
    let wb = vauchi_with_identity("Alice");
    let claim = RecoveryClaim::new(&[0x03u8; 32], &[0x04u8; 32], 0);
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

    let alice_old = Identity::create("Alice-Old", 0);
    let alice_new = Identity::create("Alice-New", 0);
    add_contact(&helper, &alice_old, "Alice");
    let before = now();

    let voucher_b64 = helper
        .create_voucher_from_claim_b64(&claim_b64(&alice_old, &alice_new, now()))
        .unwrap();

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
    assert!(
        (before..=now()).contains(&voucher.timestamp()),
        "the voucher carries the time it was signed"
    );
}

// @internal
#[test]
fn create_voucher_from_claim_b64_refuses_a_claim_for_someone_not_a_contact() {
    let helper = vauchi_with_identity("Bob");
    let stranger_old = Identity::create("Stranger-Old", 0);
    let stranger_new = Identity::create("Stranger-New", 0);

    let result =
        helper.create_voucher_from_claim_b64(&claim_b64(&stranger_old, &stranger_new, now()));

    assert!(matches!(result, Err(VauchiError::ContactNotFound(_))));
}

// @internal
#[test]
fn create_voucher_from_claim_b64_refuses_an_expired_claim() {
    let helper = vauchi_with_identity("Bob");
    let alice_old = Identity::create("Alice-Old", 0);
    let alice_new = Identity::create("Alice-New", 0);
    add_contact(&helper, &alice_old, "Alice");
    let past_the_48_hour_window = now() - RecoveryClaim::MAX_AGE_SECS - 60;

    let result = helper.create_voucher_from_claim_b64(&claim_b64(
        &alice_old,
        &alice_new,
        past_the_48_hour_window,
    ));

    assert!(matches!(result, Err(VauchiError::InvalidState(_))));
}

// @internal
#[test]
fn recovery_claim_contact_names_the_contact_whose_key_the_claim_carries() {
    let helper = vauchi_with_identity("Bob");
    let alice_old = Identity::create("Alice-Old", 0);
    let alice_new = Identity::create("Alice-New", 0);
    add_contact(&helper, &alice_old, "Alice");
    let claim = helper
        .parse_recovery_claim_b64(&claim_b64(&alice_old, &alice_new, now()))
        .unwrap();

    let contact = helper.recovery_claim_contact(&claim).unwrap();

    assert_eq!(contact.display_name(), "Alice");
    assert_eq!(contact.public_key(), Some(alice_old.signing_public_key()));
}

// @internal
#[test]
fn recovery_claim_contact_refuses_a_key_that_is_no_contact() {
    let helper = vauchi_with_identity("Bob");
    let claim = RecoveryClaim::new(&[0x33u8; 32], &[0x44u8; 32], now());

    assert!(matches!(
        helper.recovery_claim_contact(&claim),
        Err(VauchiError::ContactNotFound(_))
    ));
}

// @internal
#[test]
fn create_voucher_from_claim_b64_rejects_self_vouching() {
    // The helper IS the recovering identity → SelfVouching error.
    let alice = vauchi_with_identity("Alice");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let old = Identity::create("Alice-Old", 0);
    add_contact(&alice, &old, "Old Alice");
    let claim = RecoveryClaim::new(old.signing_public_key(), &alice_pk, now());
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
    let claim = RecoveryClaim::new(&[1u8; 32], &[2u8; 32], 0);
    let result = wb.create_voucher_from_claim_b64(&b64(&claim.to_bytes()));
    assert!(matches!(result, Err(VauchiError::IdentityNotInitialized)));
}
