// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Content Encryption Key (CEK) — crypto-shredding layer.
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Card updates use per-contact content encryption key"
//!   - "Crypto-shredding renders card unreadable without key"
//!   - "Account deletion destroys all content encryption keys"

use vauchi_core::crypto::cek::ContentEncryptionKey;

// @scenario: security.feature:Contact cards are encrypted at rest
// @scenario: privacy_compliance.feature:Card updates use per-contact content encryption key
#[test]
fn test_cek_generate_encrypt_decrypt() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"Alice Smith\nphone: +41 79 123 45 67";

    let ciphertext = cek.encrypt(plaintext).expect("encryption should succeed");
    assert_ne!(ciphertext, plaintext);

    let decrypted = cek.decrypt(&ciphertext).expect("decryption should succeed");
    assert_eq!(decrypted, plaintext);
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_different_keys_cannot_decrypt() {
    let cek1 = ContentEncryptionKey::generate();
    let cek2 = ContentEncryptionKey::generate();
    let plaintext = b"contact card data";

    let ciphertext = cek1.encrypt(plaintext).expect("encryption should succeed");
    let result = cek2.decrypt(&ciphertext);
    assert!(result.is_err(), "different CEK should not decrypt");
}

// @scenario: security.feature:Forward secrecy via Double Ratchet
#[test]
fn test_cek_rotation_invalidates_old() {
    let old_cek = ContentEncryptionKey::generate();
    let new_cek = ContentEncryptionKey::generate();
    let card_v1 = b"card version 1";
    let card_v2 = b"card version 2";

    // Encrypt v1 with old CEK
    let ciphertext_v1 = old_cek.encrypt(card_v1).unwrap();
    // Encrypt v2 with new CEK (rotation)
    let ciphertext_v2 = new_cek.encrypt(card_v2).unwrap();

    // Old CEK can still decrypt v1
    assert_eq!(old_cek.decrypt(&ciphertext_v1).unwrap(), card_v1);
    // New CEK can decrypt v2
    assert_eq!(new_cek.decrypt(&ciphertext_v2).unwrap(), card_v2);
    // New CEK cannot decrypt v1 (old content)
    assert!(new_cek.decrypt(&ciphertext_v1).is_err());
    // Old CEK cannot decrypt v2 (new content)
    assert!(old_cek.decrypt(&ciphertext_v2).is_err());
}

// @scenario: security.feature:Secure deletion of data
#[test]
fn test_cek_destroy_renders_card_unreadable() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"sensitive contact data";

    let ciphertext = cek.encrypt(plaintext).unwrap();

    // Simulate destroying the CEK by dropping it
    let key_bytes = cek.to_bytes();
    drop(cek);

    // Recreating from bytes still works (the bytes are the secret)
    let restored = ContentEncryptionKey::from_bytes(key_bytes);
    assert_eq!(restored.decrypt(&ciphertext).unwrap(), plaintext);

    // But without the bytes, ciphertext is irrecoverable
    let wrong_key = ContentEncryptionKey::generate();
    assert!(wrong_key.decrypt(&ciphertext).is_err());
}

#[test]
fn test_cek_serialization_roundtrip() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"roundtrip test data";

    let ciphertext = cek.encrypt(plaintext).unwrap();

    // Serialize and deserialize
    let bytes = cek.to_bytes();
    let restored = ContentEncryptionKey::from_bytes(bytes);

    assert_eq!(restored.decrypt(&ciphertext).unwrap(), plaintext);
}

#[test]
fn test_cek_encrypt_empty_plaintext() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"";

    let ciphertext = cek.encrypt(plaintext).unwrap();
    let decrypted = cek.decrypt(&ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_cek_encrypt_large_payload() {
    let cek = ContentEncryptionKey::generate();
    // Max card: 25 fields * 1000 chars = 25KB
    let plaintext = vec![0x42u8; 25_000];

    let ciphertext = cek.encrypt(&plaintext).unwrap();
    let decrypted = cek.decrypt(&ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}

// @scenario: security.feature:Correct algorithms used
#[test]
fn test_cek_ciphertext_is_tagged_xchacha20() {
    let cek = ContentEncryptionKey::generate();
    let ciphertext = cek.encrypt(b"test").unwrap();

    // XChaCha20-Poly1305 tag is 0x02
    assert_eq!(ciphertext[0], 0x02, "CEK should use XChaCha20-Poly1305");
}

#[test]
fn test_cek_each_encryption_produces_unique_ciphertext() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"same data";

    let ct1 = cek.encrypt(plaintext).unwrap();
    let ct2 = cek.encrypt(plaintext).unwrap();

    // Different nonces produce different ciphertext
    assert_ne!(ct1, ct2);
    // But both decrypt to the same plaintext
    assert_eq!(cek.decrypt(&ct1).unwrap(), plaintext);
    assert_eq!(cek.decrypt(&ct2).unwrap(), plaintext);
}

// --- Additional CEK coverage tests ---

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_clone_preserves_functionality() {
    let cek_original = ContentEncryptionKey::generate();
    let cek_clone = cek_original.clone();

    let plaintext = b"test data for clone";
    let ciphertext = cek_original.encrypt(plaintext).unwrap();

    // Cloned key should decrypt the same ciphertext
    let decrypted = cek_clone.decrypt(&ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}

// @scenario: security.feature:Memory dump protection
#[test]
fn test_cek_debug_redacted() {
    let cek = ContentEncryptionKey::generate();
    let debug_str = format!("{:?}", cek);

    // Key material should be redacted
    assert!(debug_str.contains("REDACTED"));
    assert!(debug_str.contains("ContentEncryptionKey"));
    // Should not contain raw key bytes
    assert!(!debug_str.contains("SymmetricKey"));
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_from_bytes_reject_all_zeros() {
    // from_bytes delegates to SymmetricKey::from_bytes which rejects all-zeros
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ContentEncryptionKey::from_bytes([0u8; 32])
    }));
    assert!(result.is_err(), "CEK should reject all-zeros key");
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_decrypt_corrupted_payload() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"sensitive data";

    let mut ciphertext = cek.encrypt(plaintext).unwrap();
    // Corrupt a byte in the middle
    let mid = ciphertext.len() / 2;
    ciphertext[mid] ^= 0xFF;

    let result = cek.decrypt(&ciphertext);
    assert!(result.is_err(), "Corrupted ciphertext must fail to decrypt");
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_decrypt_truncated_ciphertext() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"test";

    let mut ciphertext = cek.encrypt(plaintext).unwrap();
    // Truncate the ciphertext
    ciphertext.truncate(10);

    let result = cek.decrypt(&ciphertext);
    assert!(result.is_err(), "Truncated ciphertext must fail to decrypt");
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_multiple_clones_independent() {
    let cek1 = ContentEncryptionKey::generate();
    let cek2 = cek1.clone();
    let cek3 = cek1.clone();

    let plaintext = b"test";
    let ct1 = cek1.encrypt(plaintext).unwrap();
    let ct2 = cek2.encrypt(plaintext).unwrap();

    // All clones should decrypt both ciphertexts
    assert_eq!(cek1.decrypt(&ct1).unwrap(), plaintext);
    assert_eq!(cek2.decrypt(&ct1).unwrap(), plaintext);
    assert_eq!(cek3.decrypt(&ct1).unwrap(), plaintext);

    assert_eq!(cek1.decrypt(&ct2).unwrap(), plaintext);
    assert_eq!(cek2.decrypt(&ct2).unwrap(), plaintext);
    assert_eq!(cek3.decrypt(&ct2).unwrap(), plaintext);
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_to_bytes_from_bytes_consistency() {
    let cek1 = ContentEncryptionKey::generate();
    let bytes = cek1.to_bytes();
    let cek2 = ContentEncryptionKey::from_bytes(bytes);

    let plaintext = b"consistency test";
    let ct1 = cek1.encrypt(plaintext).unwrap();
    let ct2 = cek2.encrypt(plaintext).unwrap();

    // Both keys should decrypt ciphertexts from either key
    assert_eq!(cek1.decrypt(&ct1).unwrap(), plaintext);
    assert_eq!(cek1.decrypt(&ct2).unwrap(), plaintext);
    assert_eq!(cek2.decrypt(&ct1).unwrap(), plaintext);
    assert_eq!(cek2.decrypt(&ct2).unwrap(), plaintext);
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_single_byte_plaintext() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"X";

    let ciphertext = cek.encrypt(plaintext).unwrap();
    let decrypted = cek.decrypt(&ciphertext).unwrap();

    assert_eq!(decrypted, plaintext);
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_binary_plaintext_with_nulls() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = vec![0x00, 0xFF, 0x00, 0x42, 0x00];

    let ciphertext = cek.encrypt(&plaintext).unwrap();
    let decrypted = cek.decrypt(&ciphertext).unwrap();

    assert_eq!(decrypted, plaintext);
}

// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_cek_decrypt_empty_ciphertext() {
    let cek = ContentEncryptionKey::generate();
    let result = cek.decrypt(&[]);

    assert!(result.is_err(), "Empty ciphertext must fail");
}
