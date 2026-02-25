// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Additional encryption tests for coverage of AES-GCM and legacy paths

use vauchi_core::crypto::encryption::{encrypt_aes_gcm, encrypt_legacy_untagged};
use vauchi_core::crypto::{decrypt, decrypt_with_ad, encrypt, encrypt_with_ad, SymmetricKey};

// @scenario: security:Correct algorithms used
#[test]
fn test_aes_gcm_tagged_roundtrip() {
    let key = SymmetricKey::generate();
    let plaintext = b"AES-GCM tagged data";
    let ciphertext = encrypt_aes_gcm(&key, plaintext).unwrap();
    let decrypted = decrypt(&key, &ciphertext).unwrap();
    assert_eq!(plaintext.to_vec(), decrypted);
}

// @scenario: security:Correct algorithms used
#[test]
fn test_aes_gcm_tagged_starts_with_tag() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt_aes_gcm(&key, b"test").unwrap();
    assert_eq!(ciphertext[0], 0x01); // ALG_TAG_AES_GCM
}

// @scenario: security:Correct algorithms used
#[test]
fn test_xchacha20_tagged_starts_with_tag() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt(&key, b"test").unwrap();
    assert_eq!(ciphertext[0], 0x02); // ALG_TAG_XCHACHA20
}

// @scenario: security:Contact cards are encrypted at rest
#[test]
fn test_legacy_untagged_roundtrip() {
    let key = SymmetricKey::generate();
    let plaintext = b"legacy untagged data";
    let ciphertext = encrypt_legacy_untagged(&key, plaintext).unwrap();
    // Legacy format starts directly with nonce bytes (not 0x01 or 0x02)
    let decrypted = decrypt(&key, &ciphertext).unwrap();
    assert_eq!(plaintext.to_vec(), decrypted);
}

// @scenario: security:Contact cards are encrypted at rest
#[test]
fn test_decrypt_empty_ciphertext() {
    let key = SymmetricKey::generate();
    let result = decrypt(&key, &[]);
    assert!(result.is_err());
}

// @scenario: security:Contact cards are encrypted at rest
#[test]
fn test_decrypt_too_short_xchacha20() {
    let key = SymmetricKey::generate();
    // Tag 0x02 + less than 24 (nonce) + 16 (tag) bytes
    let short = vec![0x02, 0, 0, 0, 0];
    let result = decrypt(&key, &short);
    assert!(result.is_err());
}

// @scenario: security:Contact cards are encrypted at rest
#[test]
fn test_decrypt_too_short_aes_gcm() {
    let key = SymmetricKey::generate();
    // Tag 0x01 + less than 12 (nonce) + 16 (tag) bytes
    let short = vec![0x01, 0, 0, 0, 0];
    let result = decrypt(&key, &short);
    assert!(result.is_err());
}

// @scenario: security:Server cannot access plaintext
#[test]
fn test_decrypt_wrong_key() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let ciphertext = encrypt(&key1, b"secret data").unwrap();
    let result = decrypt(&key2, &ciphertext);
    assert!(result.is_err());
}

// @scenario: security:Contact cards are encrypted at rest
#[test]
fn test_decrypt_corrupted_data() {
    let key = SymmetricKey::generate();
    let mut ciphertext = encrypt(&key, b"some data").unwrap();
    // Corrupt a byte in the ciphertext
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0xFF;
    let result = decrypt(&key, &ciphertext);
    assert!(result.is_err());
}

// @scenario: security:Sufficient key lengths
#[test]
fn test_symmetric_key_generate() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

// @scenario: security:Sufficient key lengths
#[test]
fn test_symmetric_key_from_bytes() {
    let bytes = [0x42u8; 32];
    let key = SymmetricKey::from_bytes(bytes);
    assert_eq!(key.as_bytes(), &bytes);
}

// @scenario: security:Memory dump protection
#[test]
fn test_symmetric_key_debug_redacted() {
    let key = SymmetricKey::generate();
    let debug = format!("{:?}", key);
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains(&format!("{:?}", key.as_bytes())));
}

/// SP-9 #227: SymmetricKey::from_bytes rejects all-zeros (degenerate key).
// @scenario: security:No weak cryptography
#[test]
#[should_panic(expected = "all-zeros key is degenerate")]
fn test_symmetric_key_from_bytes_rejects_zeros() {
    SymmetricKey::from_bytes([0u8; 32]);
}

/// SP-9 #227: from_bytes_unchecked allows any key (for deserialization).
#[test]
fn test_symmetric_key_from_bytes_unchecked_allows_zeros() {
    let key = SymmetricKey::from_bytes_unchecked([0u8; 32]);
    assert_eq!(key.as_bytes(), &[0u8; 32]);
}

// @scenario: security:Contact cards are encrypted at rest
#[test]
fn test_large_plaintext() {
    let key = SymmetricKey::generate();
    let plaintext = vec![0xAB; 100_000];
    let ciphertext = encrypt(&key, &plaintext).unwrap();
    let decrypted = decrypt(&key, &ciphertext).unwrap();
    assert_eq!(plaintext, decrypted);
}

// @scenario: security:Server cannot access plaintext
#[test]
fn test_aes_gcm_wrong_key() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let ciphertext = encrypt_aes_gcm(&key1, b"test").unwrap();
    let result = decrypt(&key2, &ciphertext);
    assert!(result.is_err());
}

// @scenario: security:Server cannot access plaintext
#[test]
fn test_legacy_untagged_wrong_key() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let ciphertext = encrypt_legacy_untagged(&key1, b"test").unwrap();
    let result = decrypt(&key2, &ciphertext);
    assert!(result.is_err());
}

// --- AEAD associated data binding tests ---

// @scenario: security:Contact cards are encrypted in transit
#[test]
fn test_encrypt_with_ad_roundtrip() {
    let key = SymmetricKey::generate();
    let plaintext = b"message with associated data";
    let ad = b"header-context-binding";
    let ciphertext = encrypt_with_ad(&key, plaintext, ad).unwrap();
    let decrypted = decrypt_with_ad(&key, &ciphertext, ad).unwrap();
    assert_eq!(plaintext.to_vec(), decrypted);
}

// @scenario: security:Correct algorithms used
#[test]
fn test_encrypt_with_ad_tag_is_0x03() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt_with_ad(&key, b"test", b"ad").unwrap();
    assert_eq!(ciphertext[0], 0x03); // ALG_TAG_XCHACHA20_AD
}

// @scenario: security:Contact cards are encrypted in transit
#[test]
fn test_encrypt_with_ad_wrong_ad_fails() {
    let key = SymmetricKey::generate();
    let ciphertext = encrypt_with_ad(&key, b"secret", b"correct-ad").unwrap();
    let result = decrypt_with_ad(&key, &ciphertext, b"wrong-ad");
    assert!(result.is_err(), "Decryption with wrong AD must fail");
}

// @scenario: security:Contact cards are encrypted in transit
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

// @scenario: security:Contact cards are encrypted in transit
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

#[test]
fn test_decrypt_with_ad_backward_compat_tag_0x01() {
    let key = SymmetricKey::generate();
    let plaintext = b"aes message";
    let ciphertext = encrypt_aes_gcm(&key, plaintext).unwrap();
    // decrypt_with_ad should handle tag 0x01 by ignoring AD
    let decrypted = decrypt_with_ad(&key, &ciphertext, b"ignored-ad").unwrap();
    assert_eq!(plaintext.to_vec(), decrypted);
}
