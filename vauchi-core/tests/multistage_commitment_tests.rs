// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::commitment::Commitment;

#[test]
fn test_commitment_create_and_verify() {
    let plaintext = b"secret contact card data";
    let commitment = Commitment::create(plaintext);

    assert_ne!(commitment.reveal_key(), &[0u8; 32]);
    assert_ne!(commitment.ciphertext(), plaintext.as_slice());
    assert_ne!(commitment.hash(), &[0u8; 32]);

    // Verify with correct reveal key
    let decrypted = commitment.open().unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_commitment_hash_is_deterministic() {
    let plaintext = b"test data";
    let c1 = Commitment::create(plaintext);
    // Hash = SHA256(reveal_key || ciphertext) — different reveal_key each time
    // so hashes should differ between sessions
    let c2 = Commitment::create(plaintext);
    assert_ne!(c1.hash(), c2.hash()); // different reveal keys
}

#[test]
fn test_commitment_verify_from_parts() {
    let plaintext = b"alice's contact card";
    let commitment = Commitment::create(plaintext);

    let hash = commitment.hash();
    let ciphertext = commitment.ciphertext().to_vec();
    let reveal_key = *commitment.reveal_key();

    // Reconstruct and verify
    let decrypted = Commitment::open_with_key(&reveal_key, &ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);

    // Verify commitment hash
    assert!(Commitment::verify_hash(&reveal_key, &ciphertext, hash));
}

#[test]
fn test_commitment_wrong_key_fails() {
    let plaintext = b"secret";
    let commitment = Commitment::create(plaintext);
    let ciphertext = commitment.ciphertext().to_vec();
    let wrong_key = [0xFFu8; 32];

    let result = Commitment::open_with_key(&wrong_key, &ciphertext);
    assert!(result.is_err());
}

#[test]
fn test_commitment_tampered_ciphertext_fails() {
    let plaintext = b"data";
    let commitment = Commitment::create(plaintext);
    let reveal_key = *commitment.reveal_key();
    let mut ciphertext = commitment.ciphertext().to_vec();
    ciphertext[0] ^= 0xFF; // tamper

    let result = Commitment::open_with_key(&reveal_key, &ciphertext);
    assert!(result.is_err());
}

#[test]
fn test_commitment_hash_mismatch_detected() {
    let plaintext = b"data";
    let commitment = Commitment::create(plaintext);
    let ciphertext = commitment.ciphertext().to_vec();
    let reveal_key = *commitment.reveal_key();
    let wrong_hash = [0u8; 32];

    assert!(!Commitment::verify_hash(
        &reveal_key,
        &ciphertext,
        &wrong_hash
    ));
}
