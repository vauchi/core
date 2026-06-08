// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Encryption coverage tests for XChaCha20-Poly1305 paths

use vauchi_core::crypto::{SymmetricKey, decrypt, decrypt_with_ad, encrypt, encrypt_with_ad};

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn test_xchacha20_tagged_starts_with_tag() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt(&key, b"test").unwrap();
    assert_eq!(ciphertext[0], 0x02); // ALG_TAG_XCHACHA20
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_decrypt_empty_ciphertext() {
    let key = SymmetricKey::generate();
    let result = decrypt(&key, &[]);
    result.expect_err("expected error");
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_decrypt_too_short_xchacha20() {
    let key = SymmetricKey::generate();
    // Tag 0x02 + less than 24 (nonce) + 16 (tag) bytes
    let short = vec![0x02, 0, 0, 0, 0];
    let result = decrypt(&key, &short);
    result.expect_err("expected error");
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_decrypt_unknown_tag_rejected() {
    let key = SymmetricKey::generate();
    // Tag 0x01 (former AES-GCM) should now be rejected
    let short = vec![0x01, 0, 0, 0, 0];
    let result = decrypt(&key, &short);
    assert!(result.is_err(), "Unknown algorithm tag must be rejected");
}

// @scenario: security :: Server cannot access plaintext
// @internal
#[test]
fn test_decrypt_wrong_key() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let ciphertext = encrypt(&key1, b"secret data").unwrap();
    let result = decrypt(&key2, &ciphertext);
    result.expect_err("expected error");
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_decrypt_corrupted_data() {
    let key = SymmetricKey::generate();
    let mut ciphertext = encrypt(&key, b"some data").unwrap();
    // Corrupt a byte in the ciphertext
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0xFF;
    let result = decrypt(&key, &ciphertext);
    result.expect_err("expected error");
}

// @scenario: security :: Sufficient key lengths
// @internal
#[test]
fn test_symmetric_key_generate() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

// @scenario: security :: Sufficient key lengths
// @internal
#[test]
fn test_symmetric_key_from_bytes() {
    let bytes = [0x42u8; 32];
    let key = SymmetricKey::from_bytes(bytes);
    assert_eq!(key.as_bytes(), &bytes);
}

// @scenario: security :: Memory dump protection
// @internal
#[test]
fn test_symmetric_key_debug_redacted() {
    let key = SymmetricKey::generate();
    let debug = format!("{:?}", key);
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains(&format!("{:?}", key.as_bytes())));
}

/// SP-9 #227: SymmetricKey::from_bytes rejects all-zeros (degenerate key).
// @scenario: security :: No weak cryptography
// @internal
#[test]
#[should_panic(expected = "all-zeros key is degenerate")]
fn test_symmetric_key_from_bytes_rejects_zeros() {
    SymmetricKey::from_bytes([0u8; 32]);
}

/// SP-9 #227: from_bytes_unchecked allows any key (for deserialization).
// @internal
#[test]
fn test_symmetric_key_from_bytes_unchecked_allows_zeros() {
    let key = SymmetricKey::from_bytes_unchecked([0u8; 32]);
    assert_eq!(key.as_bytes(), &[0u8; 32]);
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_large_plaintext() {
    let key = SymmetricKey::generate();
    let plaintext = vec![0xAB; 100_000];
    let ciphertext = encrypt(&key, &plaintext).unwrap();
    let decrypted = decrypt(&key, &ciphertext).unwrap();
    assert_eq!(plaintext, decrypted);
}

// --- AEAD associated data binding tests ---

// @scenario: security :: Contact cards are encrypted in transit
// @internal
#[test]
fn test_encrypt_with_ad_roundtrip() {
    let key = SymmetricKey::generate();
    let plaintext = b"message with associated data";
    let ad = b"header-context-binding";
    let ciphertext = encrypt_with_ad(&key, plaintext, ad).unwrap();
    let decrypted = decrypt_with_ad(&key, &ciphertext, ad).unwrap();
    assert_eq!(plaintext.to_vec(), decrypted);
}

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn test_encrypt_with_ad_tag_is_0x03() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt_with_ad(&key, b"test", b"ad").unwrap();
    assert_eq!(ciphertext[0], 0x03); // ALG_TAG_XCHACHA20_AD
}

// @scenario: security :: Contact cards are encrypted in transit
// @internal
#[test]
fn test_encrypt_with_ad_wrong_ad_fails() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt_with_ad(&key, b"secret", b"correct-ad").unwrap();
    let result = decrypt_with_ad(&key, &ciphertext, b"wrong-ad");
    assert!(result.is_err(), "Decryption with wrong AD must fail");
}

// @scenario: security :: Contact cards are encrypted in transit
// @internal
#[test]
fn test_encrypt_with_ad_empty_ad_fails_against_non_empty() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt_with_ad(&key, b"data", b"some-ad").unwrap();
    let result = decrypt_with_ad(&key, &ciphertext, b"");
    assert!(
        result.is_err(),
        "Decryption with empty AD must fail when encrypted with non-empty AD"
    );
}

// @scenario: security :: Contact cards are encrypted in transit
// @internal
#[test]
fn test_encrypt_with_ad_cannot_decrypt_without_ad() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt_with_ad(&key, b"data", b"context").unwrap();
    // Plain decrypt() should reject tag 0x03
    let result = decrypt(&key, &ciphertext);
    assert!(
        result.is_err(),
        "Plain decrypt must reject AD-bound ciphertext"
    );
}

// @internal
#[test]
fn test_decrypt_with_ad_backward_compat_tag_0x02() {
    let key = SymmetricKey::generate();
    let plaintext = b"old message";
    // Encrypt with tag 0x02 (no AD)
    let ciphertext = encrypt(&key, plaintext).unwrap();
    // decrypt_with_ad should handle tag 0x02 by ignoring AD
    let decrypted = decrypt_with_ad(&key, &ciphertext, b"ignored-ad").unwrap();
    assert_eq!(plaintext.to_vec(), decrypted);
}

// --- Additional coverage for edge cases and error paths ---

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_encrypt_with_ad_empty_plaintext() {
    let key = SymmetricKey::generate();
    let ad = b"some associated data";
    let ciphertext = encrypt_with_ad(&key, b"", ad).unwrap();
    let decrypted = decrypt_with_ad(&key, &ciphertext, ad).unwrap();
    assert_eq!(decrypted, Vec::<u8>::new());
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_encrypt_with_ad_large_associated_data() {
    let key = SymmetricKey::generate();
    let plaintext = b"secret message";
    let large_ad = vec![0xAB; 10_000];
    let ciphertext = encrypt_with_ad(&key, plaintext, &large_ad).unwrap();
    let decrypted = decrypt_with_ad(&key, &ciphertext, &large_ad).unwrap();
    assert_eq!(plaintext.to_vec(), decrypted);
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_decrypt_with_ad_empty_ciphertext() {
    let key = SymmetricKey::generate();
    let result = decrypt_with_ad(&key, &[], b"ad");
    assert!(result.is_err(), "Empty ciphertext must be rejected");
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_decrypt_with_ad_too_short_xchacha20() {
    let key = SymmetricKey::generate();
    // Tag 0x02 + less than required minimum
    let short = vec![0x02, 0, 0];
    let result = decrypt_with_ad(&key, &short, b"ad");
    assert!(result.is_err(), "Too-short ciphertext must be rejected");
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_decrypt_with_ad_unknown_tag_rejected() {
    let key = SymmetricKey::generate();
    // Tag 0x01 (former AES-GCM) should now be rejected
    let short = vec![0x01, 0, 0];
    let result = decrypt_with_ad(&key, &short, b"ad");
    assert!(
        result.is_err(),
        "Unknown algorithm tag must be rejected in decrypt_with_ad"
    );
}

// @scenario: security :: Contact cards are encrypted in transit
// @internal
#[test]
fn test_encrypt_with_ad_empty_ad() {
    let key = SymmetricKey::generate();
    let plaintext = b"message";
    let ciphertext = encrypt_with_ad(&key, plaintext, b"").unwrap();
    let decrypted = decrypt_with_ad(&key, &ciphertext, b"").unwrap();
    assert_eq!(plaintext.to_vec(), decrypted);
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_decrypt_xchacha20_corrupted_tag() {
    let key = SymmetricKey::generate();
    let mut ciphertext = encrypt(&key, b"test").unwrap();
    // Corrupt the authentication tag (last 16 bytes)
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0xFF;
    let result = decrypt(&key, &ciphertext);
    assert!(result.is_err(), "Corrupted XChaCha20 tag must fail");
}

// @scenario: security :: Contact cards are encrypted in transit
// @internal
#[test]
fn test_decrypt_xchacha20_ad_corrupted_tag() {
    let key = SymmetricKey::generate();
    let mut ciphertext = encrypt_with_ad(&key, b"test", b"ad").unwrap();
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0xFF;
    let result = decrypt_with_ad(&key, &ciphertext, b"ad");
    assert!(result.is_err(), "Corrupted XChaCha20-AD tag must fail");
}

// @scenario: security :: Contact cards are encrypted in transit
// @internal
#[test]
fn test_encrypt_with_ad_nonce_determinism() {
    let key = SymmetricKey::generate();
    let plaintext = b"test";
    let ad = b"ad";
    let ciphertext1 = encrypt_with_ad(&key, plaintext, ad).unwrap();
    let ciphertext2 = encrypt_with_ad(&key, plaintext, ad).unwrap();
    // Each encryption should produce a different ciphertext (due to random nonce)
    assert_ne!(
        ciphertext1, ciphertext2,
        "Random nonces should produce different ciphertexts"
    );
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_unrecognized_tag_rejected() {
    let key = SymmetricKey::generate();
    let mut fake_ciphertext = vec![0xFF]; // Invalid tag
    fake_ciphertext.extend_from_slice(&[0; 12]); // padding
    fake_ciphertext.extend_from_slice(&[0; 16]); // padding
    let result = decrypt(&key, &fake_ciphertext);
    assert!(result.is_err(), "Unrecognized tag must be rejected");
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_symmetric_key_from_bytes_single_bit_nonzero() {
    // Test that a key with only a single bit set is accepted
    let mut bytes = [0u8; 32];
    bytes[0] = 1; // Single bit in first byte
    let key = SymmetricKey::from_bytes(bytes);
    assert_eq!(key.as_bytes()[0], 1);
}

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_encrypt_decrypt_minimum_ciphertext_size() {
    let key = SymmetricKey::generate();
    // Minimum ciphertext for XChaCha20: 1 (tag) + 24 (nonce) + 16 (auth tag) = 41 bytes
    let ciphertext = encrypt(&key, &[]).unwrap();
    assert!(
        ciphertext.len() >= 41,
        "Minimum ciphertext size should be at least 41 bytes"
    );
    let decrypted = decrypt(&key, &ciphertext).unwrap();
    assert_eq!(decrypted, Vec::<u8>::new());
}
