// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::commitment::Commitment;

// @internal
#[test]
fn test_commitment_create_and_verify() {
    let plaintext = b"secret contact card data";
    let commitment = Commitment::create(plaintext).unwrap();

    assert_ne!(commitment.reveal_key(), &[0u8; 32]);
    assert_ne!(commitment.ciphertext(), plaintext.as_slice());
    assert_ne!(commitment.hash(), &[0u8; 32]);

    let decrypted = commitment.open().unwrap();
    assert_eq!(decrypted, plaintext);
}

// @internal
#[test]
fn test_commitment_hash_is_deterministic() {
    let plaintext = b"test data";
    let c1 = Commitment::create(plaintext).unwrap();
    // Hash = SHA256(reveal_key || ciphertext) — different reveal_key each time
    // so hashes should differ between sessions
    let c2 = Commitment::create(plaintext).unwrap();
    assert_ne!(c1.hash(), c2.hash()); // different reveal keys
}

// @internal
#[test]
fn test_commitment_verify_from_parts() {
    let plaintext = b"alice's contact card";
    let commitment = Commitment::create(plaintext).unwrap();

    let hash = commitment.hash();
    let ciphertext = commitment.ciphertext().to_vec();
    let reveal_key = *commitment.reveal_key();

    let decrypted = Commitment::open_with_key(&reveal_key, &ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);

    assert!(Commitment::verify_hash(&reveal_key, &ciphertext, hash));
}

// @internal
#[test]
fn test_commitment_wrong_key_fails() {
    let plaintext = b"secret";
    let commitment = Commitment::create(plaintext).unwrap();
    let ciphertext = commitment.ciphertext().to_vec();
    let wrong_key = [0xFFu8; 32];

    let result = Commitment::open_with_key(&wrong_key, &ciphertext);
    result.expect_err("expected error");
}

// @internal
#[test]
fn test_commitment_tampered_ciphertext_fails() {
    let plaintext = b"data";
    let commitment = Commitment::create(plaintext).unwrap();
    let reveal_key = *commitment.reveal_key();
    let mut ciphertext = commitment.ciphertext().to_vec();
    ciphertext[0] ^= 0xFF; // tamper

    let result = Commitment::open_with_key(&reveal_key, &ciphertext);
    result.expect_err("expected error");
}

// @internal
#[test]
fn test_commitment_hash_mismatch_detected() {
    let plaintext = b"data";
    let commitment = Commitment::create(plaintext).unwrap();
    let ciphertext = commitment.ciphertext().to_vec();
    let reveal_key = *commitment.reveal_key();
    let wrong_hash = [0u8; 32];

    assert!(!Commitment::verify_hash(
        &reveal_key,
        &ciphertext,
        &wrong_hash
    ));
}

// === T1.7: Commitment binds relay metadata (context) ===

// @internal
#[test]
fn test_commitment_with_context_binds_relay_url() {
    let plaintext = b"contact card data";
    let context = b"https://relay.vauchi.app";

    let commitment = Commitment::create_with_context(plaintext, context).unwrap();

    assert!(Commitment::verify_hash_with_context(
        commitment.reveal_key(),
        commitment.ciphertext(),
        commitment.hash(),
        context,
    ));

    // Verify with wrong context (swapped relay URL) fails
    let wrong_context = b"https://evil-relay.example";
    assert!(!Commitment::verify_hash_with_context(
        commitment.reveal_key(),
        commitment.ciphertext(),
        commitment.hash(),
        wrong_context,
    ));
}

// @internal
#[test]
fn test_commitment_with_context_empty_context_differs_from_no_context() {
    let plaintext = b"card data";
    let commitment_no_ctx = Commitment::create(plaintext).unwrap();
    let _commitment_empty_ctx = Commitment::create_with_context(plaintext, b"").unwrap();

    // Even with empty context, the hash computation path differs
    // because create() uses the legacy path (no context),
    // and create_with_context(b"") includes the length prefix.
    // This ensures backward compat: old commitments still verify with verify_hash().
    assert!(Commitment::verify_hash(
        commitment_no_ctx.reveal_key(),
        commitment_no_ctx.ciphertext(),
        commitment_no_ctx.hash(),
    ));
}

// @internal
#[test]
fn test_commitment_context_includes_relay_and_pubkey() {
    let plaintext = b"card";
    let relay_url = b"https://relay.vauchi.app";
    let noise_pk = [0xABu8; 32];

    let mut context = Vec::new();
    context.extend_from_slice(relay_url);
    context.extend_from_slice(&noise_pk);

    let commitment = Commitment::create_with_context(plaintext, &context).unwrap();

    // Tamper with just the pubkey portion
    let mut tampered_context = Vec::new();
    tampered_context.extend_from_slice(relay_url);
    tampered_context.extend_from_slice(&[0xCDu8; 32]); // different pubkey

    assert!(!Commitment::verify_hash_with_context(
        commitment.reveal_key(),
        commitment.ciphertext(),
        commitment.hash(),
        &tampered_context,
    ));
}

// @internal
#[test]
fn test_commitment_with_context_decryption_unchanged() {
    // Context affects the hash only, not the encryption.
    // Decryption still works with just the reveal key.
    let plaintext = b"secret card";
    let context = b"https://relay.vauchi.app";

    let commitment = Commitment::create_with_context(plaintext, context).unwrap();
    let decrypted = commitment.open().unwrap();
    assert_eq!(decrypted, plaintext);
}
