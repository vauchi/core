// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TDD Tests for Cryptographic Primitives
//!
//! These tests are written FIRST (RED phase) before implementation.
//! Each test maps to scenarios from security.feature and identity_management.feature

use vauchi_core::crypto::{decrypt, encrypt, SigningKeyPair, SymmetricKey};

// =============================================================================
// Ed25519 Keypair Generation Tests (from identity_management.feature)
// Scenario: Create new identity on first launch
// =============================================================================

/// Tests that a new Ed25519 keypair can be generated
/// Maps to: "Then a new Ed25519 keypair should be generated"
// @scenario: identity_management.feature:Create new identity on first launch
// @scenario: security.feature:Sufficient key lengths
#[test]
fn test_generate_ed25519_keypair_succeeds() {
    let keypair = SigningKeyPair::generate();
    let public_key = keypair.public_key();
    assert_eq!(
        public_key.as_bytes().len(),
        32,
        "Ed25519 public key should be 32 bytes"
    );
}

/// Tests that generated keypairs are unique (different each time)
/// This ensures proper randomness in key generation
// @scenario: security.feature:Sufficient key lengths
#[test]
fn test_generate_ed25519_keypair_unique() {
    let keypair1 = SigningKeyPair::generate();
    let keypair2 = SigningKeyPair::generate();

    // Two generated keypairs should be different
    assert_ne!(
        keypair1.public_key().as_bytes(),
        keypair2.public_key().as_bytes(),
        "Generated keypairs should be unique"
    );
}

/// Tests that keypair can be regenerated deterministically from seed
/// Maps to: backup/restore scenarios - "keypairs should be regenerated from the master seed"
// @scenario: identity_management.feature:Restore identity from backup
#[test]
fn test_keypair_from_seed_deterministic() {
    let seed = [42u8; 32]; // Fixed seed for testing

    let keypair1 = SigningKeyPair::from_seed(&seed);
    let keypair2 = SigningKeyPair::from_seed(&seed);

    // Same seed should produce same keypair
    assert_eq!(
        keypair1.public_key().as_bytes(),
        keypair2.public_key().as_bytes(),
        "Same seed should produce identical keypair"
    );
}

/// Tests that public key has correct length (32 bytes for Ed25519)
// @scenario: security.feature:Sufficient key lengths
#[test]
fn test_public_key_correct_length() {
    let keypair = SigningKeyPair::generate();
    let public_key = keypair.public_key();

    assert_eq!(
        public_key.as_bytes().len(),
        32,
        "Ed25519 public key should be 32 bytes"
    );
}

/// Tests that public key can be converted to human-readable fingerprint
/// Maps to: "I should see my public key fingerprint"
// @scenario: identity_management.feature:Identity verification via public key fingerprint
#[test]
fn test_public_key_fingerprint() {
    let keypair = SigningKeyPair::generate();
    let public_key = keypair.public_key();

    let fingerprint = public_key.fingerprint();

    // Fingerprint should be non-empty hex string
    assert!(!fingerprint.is_empty(), "Fingerprint should not be empty");
    // Fingerprint should only contain hex characters
    assert!(
        fingerprint.chars().all(|c| c.is_ascii_hexdigit()),
        "Fingerprint should be hexadecimal"
    );
}

// =============================================================================
// Digital Signature Tests (from security.feature)
// Scenario: Contact card signatures verified
// =============================================================================

/// Tests that signing a message produces a valid signature
// @scenario: security.feature:Contact card signatures verified
// @scenario: security.feature:Sufficient key lengths
#[test]
fn test_sign_message_produces_signature() {
    let keypair = SigningKeyPair::generate();
    let message = b"Hello, Vauchi!";

    let signature = keypair.sign(message);

    // Signature should have correct length (64 bytes for Ed25519)
    assert_eq!(
        signature.as_bytes().len(),
        64,
        "Ed25519 signature should be 64 bytes"
    );
}

/// Tests that valid signature verifies correctly
/// Maps to: "Contact card signatures verified"
// @scenario: security.feature:Contact card signatures verified
#[test]
fn test_verify_valid_signature_succeeds() {
    let keypair = SigningKeyPair::generate();
    let message = b"Important data";

    let signature = keypair.sign(message);
    let is_valid = keypair.public_key().verify(message, &signature);

    assert!(is_valid, "Valid signature should verify");
}

/// Tests that signature verification fails with wrong public key
/// Maps to: "Man-in-the-middle detection during exchange"
// @scenario: security.feature:Man-in-the-middle detection during exchange
// @scenario: security.feature:Contact card signatures verified
#[test]
fn test_verify_signature_wrong_key_fails() {
    let keypair1 = SigningKeyPair::generate();
    let keypair2 = SigningKeyPair::generate();
    let message = b"Sensitive message";

    let signature = keypair1.sign(message);
    let is_valid = keypair2.public_key().verify(message, &signature);

    assert!(!is_valid, "Signature should not verify with wrong key");
}

/// Tests that signature verification fails with tampered message
/// Maps to: "Verify update signatures" / integrity protection
// @scenario: security.feature:Update signatures verified
#[test]
fn test_verify_signature_tampered_message_fails() {
    let keypair = SigningKeyPair::generate();
    let original_message = b"Original message";
    let tampered_message = b"Tampered message";

    let signature = keypair.sign(original_message);
    let is_valid = keypair.public_key().verify(tampered_message, &signature);

    assert!(
        !is_valid,
        "Signature should not verify with tampered message"
    );
}

// =============================================================================
// Symmetric Encryption Tests (from security.feature)
// Scenario: Contact cards are encrypted at rest / in transit
// =============================================================================

/// Tests that encrypt/decrypt roundtrip produces original data
/// Maps to: "Contact cards are encrypted at rest"
// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_encrypt_decrypt_roundtrip() {
    let key = SymmetricKey::generate();
    let plaintext = b"Sensitive contact data";

    let ciphertext = encrypt(&key, plaintext).expect("Encryption should succeed");
    let decrypted = decrypt(&key, &ciphertext).expect("Decryption should succeed");

    assert_eq!(
        plaintext.to_vec(),
        decrypted,
        "Decrypted data should match original"
    );
}

/// Tests that ciphertext is different from plaintext
// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_ciphertext_differs_from_plaintext() {
    let key = SymmetricKey::generate();
    let plaintext = b"Secret message";

    let ciphertext = encrypt(&key, plaintext).expect("Encryption should succeed");

    assert_ne!(
        plaintext.to_vec(),
        ciphertext,
        "Ciphertext should differ from plaintext"
    );
}

/// Tests that decryption with wrong key fails
/// Maps to: Security - only authorized users can decrypt
// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_decrypt_wrong_key_fails() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let plaintext = b"Private data";

    let ciphertext = encrypt(&key1, plaintext).expect("Encryption should succeed");
    let result = decrypt(&key2, &ciphertext);

    assert!(result.is_err(), "Decryption with wrong key should fail");
}

/// Tests that tampered ciphertext is rejected
/// Maps to: Data integrity protection
// @scenario: security.feature:Contact cards are encrypted at rest
#[test]
fn test_decrypt_tampered_ciphertext_fails() {
    let key = SymmetricKey::generate();
    let plaintext = b"Important data";

    let mut ciphertext = encrypt(&key, plaintext).expect("Encryption should succeed");

    // Tamper with the ciphertext
    if let Some(byte) = ciphertext.last_mut() {
        *byte ^= 0xFF;
    }

    let result = decrypt(&key, &ciphertext);

    assert!(result.is_err(), "Decryption of tampered data should fail");
}

/// Tests that same plaintext produces different ciphertext each time (due to random nonce)
// @scenario: security.feature:Sufficient key lengths
#[test]
fn test_encryption_uses_random_nonce() {
    let key = SymmetricKey::generate();
    let plaintext = b"Same message";

    let ciphertext1 = encrypt(&key, plaintext).expect("Encryption should succeed");
    let ciphertext2 = encrypt(&key, plaintext).expect("Encryption should succeed");

    assert_ne!(
        ciphertext1, ciphertext2,
        "Same plaintext should produce different ciphertext (random nonce)"
    );
}

/// Tracker #226: Nonce entropy assertion.
///
/// Extracts the 24-byte nonce from XChaCha20-Poly1305 ciphertext and asserts
/// it is not degenerate (not all-zero, not all-same-byte). A degenerate nonce
/// would indicate a broken or uninitialized RNG, which would be catastrophic
/// for encryption security (nonce reuse → plaintext recovery).
///
/// Ciphertext format: `0x02 || nonce (24 bytes) || ciphertext || tag`
// @scenario: security.feature:Sufficient key lengths
#[test]
fn test_nonce_bytes_have_entropy() {
    let key = SymmetricKey::generate();
    let plaintext = b"Nonce entropy test";

    for _ in 0..10 {
        let ciphertext = encrypt(&key, plaintext).expect("Encryption should succeed");

        // Extract nonce: skip algorithm tag (1 byte), take 24 bytes
        assert!(
            ciphertext.len() > 25,
            "Ciphertext too short to contain nonce"
        );
        let nonce = &ciphertext[1..25];

        // Nonce must not be all zeros (degenerate RNG)
        assert!(
            nonce.iter().any(|&b| b != 0),
            "Nonce must not be all zeros — indicates broken RNG"
        );

        // Nonce must not be all the same byte (stuck RNG)
        let first = nonce[0];
        assert!(
            nonce.iter().any(|&b| b != first),
            "Nonce bytes must not all be identical ({:#04x}) — indicates stuck RNG",
            first
        );
    }
}

/// Tracker #226: Nonce uniqueness across encryptions.
///
/// Verifies that nonces extracted from 100 consecutive encryptions are all unique.
/// With 24-byte (192-bit) nonces from SystemRandom, collision probability is
/// negligible (~2^-128 for 100 samples). A collision here would indicate a
/// catastrophic RNG failure.
// @scenario: security.feature:Sufficient key lengths
#[test]
fn test_nonce_uniqueness_across_encryptions() {
    let key = SymmetricKey::generate();
    let plaintext = b"Nonce uniqueness test";

    let mut nonces: Vec<Vec<u8>> = Vec::with_capacity(100);
    for _ in 0..100 {
        let ciphertext = encrypt(&key, plaintext).expect("Encryption should succeed");
        let nonce = ciphertext[1..25].to_vec();
        nonces.push(nonce);
    }

    // All nonces must be unique
    for i in 0..nonces.len() {
        for j in (i + 1)..nonces.len() {
            assert_ne!(
                nonces[i], nonces[j],
                "Nonce collision at indices {} and {} — catastrophic RNG failure",
                i, j
            );
        }
    }
}

/// Tests encryption of empty data
#[test]
fn test_encrypt_empty_data() {
    let key = SymmetricKey::generate();
    let plaintext = b"";

    let ciphertext = encrypt(&key, plaintext).expect("Encryption of empty data should succeed");
    let decrypted = decrypt(&key, &ciphertext).expect("Decryption should succeed");

    assert_eq!(
        decrypted,
        plaintext.to_vec(),
        "Empty data roundtrip should work"
    );
}
