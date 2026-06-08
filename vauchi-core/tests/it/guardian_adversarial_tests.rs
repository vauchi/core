// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial payload tests for guardian token and sealed-box parsers (CC-14).
//!
//! These test security boundary functions with malformed, truncated,
//! oversized, and crafted inputs to ensure graceful error handling.
//!
//! Traces to: features/contact_recovery.feature
//! - @recovery @security: adversarial input handling

use rand::rngs::OsRng;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::RecoveryError;
use vauchi_core::recovery::guardian::GuardianToken;
use vauchi_core::recovery::sealed_box;

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------

fn generate_x25519_keypair() -> (StaticSecret, X25519PublicKey) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = X25519PublicKey::from(&secret);
    (secret, public)
}

fn valid_sealed_blob(recipient_pk: &X25519PublicKey) -> Vec<u8> {
    sealed_box::seal(b"test payload", recipient_pk)
}

// ---------------------------------------------------------------------------
// GuardianToken::from_bytes — adversarial inputs
// ---------------------------------------------------------------------------

// @scenario: contact_recovery :: Reject empty input for GuardianToken deserialization
#[test]
fn test_guardian_token_from_bytes_empty() {
    let result = GuardianToken::from_bytes(&[]);

    assert!(result.is_err(), "empty input must be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::SerializationError(_)),
        "expected SerializationError, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject single zero byte for GuardianToken deserialization
#[test]
fn test_guardian_token_from_bytes_single_zero_byte() {
    let result = GuardianToken::from_bytes(&[0x00]);

    assert!(result.is_err(), "single-byte input must be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::SerializationError(_)),
        "expected SerializationError, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject plausible-length zero buffer for GuardianToken deserialization
#[test]
fn test_guardian_token_from_bytes_127_zero_bytes() {
    let input = vec![0u8; 127];
    let result = GuardianToken::from_bytes(&input);

    assert!(
        result.is_err(),
        "plausible-length zero buffer must be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::SerializationError(_)),
        "expected SerializationError, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject or invalidate one-megabyte zero buffer for GuardianToken
#[test]
fn test_guardian_token_from_bytes_one_megabyte_zeros() {
    // postcard may successfully decode a 1 MB zero buffer into a GuardianToken
    // (zero bytes map to zero-valued fields). Either the parse must fail, or the
    // resulting token must not verify — an all-zero Ed25519 key/signature is
    // cryptographically invalid and must never be accepted as authentic.
    let input = vec![0u8; 1024 * 1024];
    match GuardianToken::from_bytes(&input) {
        Err(err) => {
            assert!(
                matches!(err, RecoveryError::SerializationError(_)),
                "expected SerializationError for 1 MB zero buffer, got: {err:?}"
            );
        }
        Ok(token) => {
            assert!(
                !token.verify(),
                "a token decoded from 1 MB of zeros must not verify \
                 (all-zero Ed25519 key/signature is cryptographically invalid)"
            );
        }
    }
}

// @scenario: contact_recovery :: Reject or fail verification when last byte is flipped in GuardianToken
#[test]
fn test_guardian_token_from_bytes_last_byte_flipped_fails_verification() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();
    let token = GuardianToken::create(&designator, guardian.public_key(), 0);
    let mut bytes = token.to_bytes();

    // Flip the last byte — corrupts the signature
    let last = bytes
        .last_mut()
        .expect("serialized token must be non-empty");
    *last ^= 0xFF;

    // Either deserialization fails or the signature fails to verify
    match GuardianToken::from_bytes(&bytes) {
        Err(err) => {
            assert!(
                matches!(err, RecoveryError::SerializationError(_)),
                "expected SerializationError on corrupted bytes, got: {err:?}"
            );
        }
        Ok(corrupted) => {
            assert!(
                !corrupted.verify(),
                "token with a flipped trailing byte must not verify"
            );
        }
    }
}

// @scenario: contact_recovery :: Reject 200 bytes of 0xFF as garbage for GuardianToken deserialization
#[test]
fn test_guardian_token_from_bytes_all_0xff_200_bytes() {
    let input = vec![0xFFu8; 200];
    let result = GuardianToken::from_bytes(&input);

    assert!(
        result.is_err(),
        "200 bytes of 0xFF must be rejected as garbage"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::SerializationError(_)),
        "expected SerializationError, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject truncated GuardianToken bytes
#[test]
fn test_guardian_token_from_bytes_half_length_truncated() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();
    let token = GuardianToken::create(&designator, guardian.public_key(), 0);
    let bytes = token.to_bytes();

    let truncated = &bytes[..bytes.len() / 2];
    let result = GuardianToken::from_bytes(truncated);

    assert!(
        result.is_err(),
        "truncated-to-half token bytes must be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::SerializationError(_)),
        "expected SerializationError, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// sealed_box::open — adversarial inputs
// ---------------------------------------------------------------------------

// @scenario: contact_recovery :: Reject empty input for sealed-box decryption
#[test]
fn test_sealed_box_open_empty_slice() {
    let (secret, _) = generate_x25519_keypair();
    let result = sealed_box::open(&[], &secret);

    assert!(result.is_err(), "empty slice must be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::InvalidFormat),
        "expected InvalidFormat for empty input, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject sub-minimum-length input for sealed-box decryption
#[test]
fn test_sealed_box_open_71_bytes_too_short() {
    let (secret, _) = generate_x25519_keypair();
    // Minimum valid length is 72 (32 + 24 + 16). 71 is one byte too short.
    let input = vec![0u8; 71];
    let result = sealed_box::open(&input, &secret);

    assert!(
        result.is_err(),
        "71-byte input must be rejected (below minimum 72)"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::InvalidFormat),
        "expected InvalidFormat for sub-minimum-length input, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject all-zero minimum-size blob in sealed-box decryption
#[test]
fn test_sealed_box_open_72_zero_bytes_decryption_fails() {
    let (secret, _) = generate_x25519_keypair();
    // Exactly minimum length — passes the format check but cannot decrypt.
    let input = vec![0u8; 72];
    let result = sealed_box::open(&input, &secret);

    assert!(result.is_err(), "72 zero bytes must fail decryption");
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::DecryptionFailed),
        "expected DecryptionFailed for all-zero minimum-size blob, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject oversized junk blob in sealed-box decryption
#[test]
fn test_sealed_box_open_one_megabyte_random_looking() {
    let (secret, _) = generate_x25519_keypair();
    // 1 MB of 0xAB bytes — passes format check (too long ≥ 72), decryption must fail.
    let input = vec![0xABu8; 1024 * 1024];
    let result = sealed_box::open(&input, &secret);

    assert!(result.is_err(), "1 MB of junk must fail to decrypt");
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::DecryptionFailed),
        "expected DecryptionFailed for oversized junk blob, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject sealed-box blob with zeroed nonce
#[test]
fn test_sealed_box_open_nonce_zeroed_fails() {
    let (secret, public) = generate_x25519_keypair();
    let mut blob = valid_sealed_blob(&public);

    // Bytes 32..56 are the nonce; zero them out.
    for b in &mut blob[32..56] {
        *b = 0;
    }

    let result = sealed_box::open(&blob, &secret);

    assert!(
        result.is_err(),
        "blob with zeroed nonce must fail to decrypt"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::DecryptionFailed),
        "expected DecryptionFailed for zeroed nonce, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject sealed-box blob with zeroed ephemeral public key
#[test]
fn test_sealed_box_open_ephemeral_pk_zeroed_fails() {
    let (secret, public) = generate_x25519_keypair();
    let mut blob = valid_sealed_blob(&public);

    // Bytes 0..32 are the ephemeral pk; zero them out.
    for b in &mut blob[..32] {
        *b = 0;
    }

    let result = sealed_box::open(&blob, &secret);

    assert!(
        result.is_err(),
        "blob with zeroed ephemeral pk must fail to decrypt"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::DecryptionFailed),
        "expected DecryptionFailed for zeroed ephemeral pk, got: {err:?}"
    );
}

// @scenario: contact_recovery :: Reject sealed-box blob with flipped authentication tag
#[test]
fn test_sealed_box_open_tag_bytes_flipped_fails() {
    let (secret, public) = generate_x25519_keypair();
    let mut blob = valid_sealed_blob(&public);

    // The last 16 bytes are the authentication tag; flip them.
    let len = blob.len();
    for b in &mut blob[len - 16..] {
        *b ^= 0xFF;
    }

    let result = sealed_box::open(&blob, &secret);

    assert!(
        result.is_err(),
        "blob with flipped authentication tag must fail"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, RecoveryError::DecryptionFailed),
        "expected DecryptionFailed for flipped tag, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// GuardianToken::verify — adversarial scenarios
// ---------------------------------------------------------------------------

// @scenario: contact_recovery :: Reject token with all-zero designator key
#[test]
fn test_guardian_token_verify_all_zero_designator_pk_returns_false() {
    let guardian = SigningKeyPair::generate();
    // We use create_with_claimed_pk to plant a zeroed claimed pk.
    let fake_signer = SigningKeyPair::generate();
    let zero_pk = vauchi_core::crypto::PublicKey::from_bytes([0u8; 32]);

    let token =
        GuardianToken::create_with_claimed_pk(&fake_signer, zero_pk, guardian.public_key(), 0);

    assert!(
        !token.verify(),
        "token with all-zero designator_pk must not verify"
    );
}

// @scenario: contact_recovery :: Reject token with all-zero guardian key tampered after signing
#[test]
fn test_guardian_token_verify_all_zero_guardian_pk_returns_false() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();

    let mut token = GuardianToken::create(&designator, guardian.public_key(), 0);
    token.set_guardian_pk_for_testing(&[0u8; 32]);

    assert!(
        !token.verify(),
        "token with all-zero guardian_pk (tampered after signing) must not verify"
    );
}

// @scenario: contact_recovery :: Reject token with zeroed signature bytes
#[test]
fn test_guardian_token_verify_all_zero_signature_returns_false() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();

    let token = GuardianToken::create(&designator, guardian.public_key(), 0);
    let mut bytes = token.to_bytes();

    // The token serialises as: designator_pk (32) + guardian_pk (32) + created_at (varint)
    // + signature (64). The signature is at the end. Find it by working with the raw
    // `signature_bytes()` approach: reconstruct via known field layout.
    // Easier: zero bytes that correspond to the signature by examining the trailing 64 bytes.
    let len = bytes.len();
    assert!(
        len >= 64,
        "serialized token must be at least 64 bytes (signature alone)"
    );
    // Zero the last 64 bytes (postcard packs fields in declaration order,
    // signature is the last field).
    for b in &mut bytes[len - 64..] {
        *b = 0;
    }

    match GuardianToken::from_bytes(&bytes) {
        Err(_) => {
            // Acceptable — postcard may reject the tampered bytes.
        }
        Ok(tampered) => {
            assert!(
                !tampered.verify(),
                "token with zeroed signature bytes must not verify"
            );
        }
    }
}
