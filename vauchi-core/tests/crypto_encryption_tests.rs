// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for crypto::encryption
//! Extracted from encryption.rs

use vauchi_core::crypto::*;

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_basic_roundtrip() {
    let key = SymmetricKey::generate();
    let data = b"test data";
    let encrypted = encrypt(&key, data).unwrap();
    let decrypted = decrypt(&key, &encrypted).unwrap();
    assert_eq!(data.to_vec(), decrypted);
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_empty_data() {
    let key = SymmetricKey::generate();
    let data = b"";
    let encrypted = encrypt(&key, data).unwrap();
    let decrypted = decrypt(&key, &encrypted).unwrap();
    assert_eq!(data.to_vec(), decrypted);
}

// --- SP-9 #227: SymmetricKey::from_bytes validation ---

// @scenario: security.feature:Sufficient key lengths
#[test]
#[should_panic(expected = "all-zeros key is degenerate")]
fn test_symmetric_key_rejects_all_zeros() {
    let _ = SymmetricKey::from_bytes([0u8; 32]);
}

// @scenario: security.feature:Sufficient key lengths
#[test]
fn test_symmetric_key_accepts_nonzero() {
    let mut bytes = [0u8; 32];
    bytes[31] = 1; // Single nonzero byte is sufficient
    let key = SymmetricKey::from_bytes(bytes);
    assert_eq!(key.as_bytes()[31], 1);
}

// --- SP-9 #231: Algorithm tag swap does not produce valid decryption ---

// @scenario: security.feature:Correct algorithms used
#[test]
fn test_algorithm_tag_swap_fails_decryption() {
    let key = SymmetricKey::generate();
    let data = b"secret data";

    let encrypted = encrypt(&key, data).unwrap();
    // encrypted[0] should be 0x02 (XChaCha20-Poly1305)
    assert_eq!(encrypted[0], 0x02, "Default encrypt uses XChaCha20 tag");

    // Swap algorithm tag to AES-GCM (0x01) — nonce size mismatch makes this fail
    let mut tampered = encrypted.clone();
    tampered[0] = 0x01;
    let result = decrypt(&key, &tampered);
    assert!(
        result.is_err(),
        "Swapping algorithm tag from XChaCha20 to AES-GCM must fail"
    );
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_encrypt_with_ad_prevents_ad_mismatch() {
    let key = SymmetricKey::generate();
    let data = b"bound data";
    let ad = b"context A";

    let encrypted = encrypt_with_ad(&key, data, ad).unwrap();

    // Correct AD decrypts fine
    let decrypted = decrypt_with_ad(&key, &encrypted, ad).unwrap();
    assert_eq!(data.as_slice(), decrypted.as_slice());

    // Wrong AD fails authentication
    let result = decrypt_with_ad(&key, &encrypted, b"context B");
    assert!(result.is_err(), "Wrong AD must fail AEAD authentication");
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_ad_bound_ciphertext_cannot_use_plain_decrypt() {
    let key = SymmetricKey::generate();
    let data = b"ad-bound";
    let ad = b"some context";

    let encrypted = encrypt_with_ad(&key, data, ad).unwrap();
    assert_eq!(encrypted[0], 0x03, "encrypt_with_ad uses tag 0x03");

    // Plain decrypt (without AD) should fail for AD-bound ciphertext
    let result = decrypt(&key, &encrypted);
    assert!(
        result.is_err(),
        "AD-bound ciphertext must not decrypt without AD"
    );
}

// --- SP-9 #234: HKDF derive_key vs derive flexibility ---

// @scenario: security.feature:Correct algorithms used
#[test]
fn test_hkdf_derive_key_produces_32_bytes() {
    let key = HKDF::derive_key(Some(b"salt"), b"ikm", b"info");
    assert_eq!(key.len(), 32);
}

#[test]
fn test_hkdf_derive_variable_length() {
    let result16 = HKDF::derive(Some(b"salt"), b"ikm", b"info", 16).unwrap();
    assert_eq!(result16.len(), 16);

    let result64 = HKDF::derive(Some(b"salt"), b"ikm", b"info", 64).unwrap();
    assert_eq!(result64.len(), 64);

    // derive_key matches first 32 bytes of derive(32)
    let result32 = HKDF::derive(Some(b"salt"), b"ikm", b"info", 32).unwrap();
    let key32 = HKDF::derive_key(Some(b"salt"), b"ikm", b"info");
    assert_eq!(result32, key32.to_vec());
}

#[test]
fn test_hkdf_different_info_different_keys() {
    let key_a = HKDF::derive_key(Some(b"salt"), b"ikm", b"domain A");
    let key_b = HKDF::derive_key(Some(b"salt"), b"ikm", b"domain B");
    assert_ne!(key_a, key_b, "Different info must produce different keys");
}

#[test]
fn test_hkdf_different_salt_different_keys() {
    let key_a = HKDF::derive_key(Some(b"salt1"), b"ikm", b"info");
    let key_b = HKDF::derive_key(Some(b"salt2"), b"ikm", b"info");
    assert_ne!(key_a, key_b, "Different salt must produce different keys");
}

// --- SP-9 #233: Constant-time comparison is used where it matters ---
// (Verified by code audit: subtle::ConstantTimeEq is used in app_password.rs,
//  exchange/audio.rs, and exchange/proximity.rs for secret comparisons.
//  Public key comparisons correctly use standard == since they are non-secret.)

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_wrong_key_cannot_decrypt() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let data = b"secret";

    let encrypted = encrypt(&key1, data).unwrap();
    let result = decrypt(&key2, &encrypted);
    assert!(result.is_err(), "Wrong key must fail decryption");
}
