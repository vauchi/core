// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for sealed-box encryption (ephemeral X25519 + XChaCha20-Poly1305).

use rand_core::OsRng;
use rstest::rstest;
use x25519_dalek::{PublicKey, StaticSecret};

use vauchi_core::recovery::{RecoveryError, sealed_box};

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------

fn generate_keypair() -> (StaticSecret, PublicKey) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret, public)
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------

// @scenario: contact_recovery :: Seal and unseal a payload with the correct recipient key
#[test]
fn test_seal_unseal_roundtrip() {
    let (secret, public) = generate_keypair();
    let plaintext = b"guardian token payload for social recovery";

    let sealed =
        sealed_box::seal(plaintext, &public).expect("seal succeeds for a valid recipient key");
    let opened = sealed_box::open(&sealed, &secret).expect("open must succeed");

    assert_eq!(
        opened, plaintext,
        "opened plaintext must equal original plaintext"
    );
}

// @scenario: contact_recovery :: Wrong recipient key cannot decrypt sealed payload
#[test]
fn test_wrong_key_cannot_decrypt() {
    let (_secret_a, public_a) = generate_keypair();
    let (secret_b, _public_b) = generate_keypair();

    let plaintext = b"secret guardian token";
    let sealed =
        sealed_box::seal(plaintext, &public_a).expect("seal succeeds for a valid recipient key");

    let result = sealed_box::open(&sealed, &secret_b);
    assert!(
        result.is_err(),
        "opening with a different secret key must fail"
    );
}

// @scenario: contact_recovery :: Tampered sealed-box ciphertext fails authentication
#[test]
fn test_tampered_ciphertext_fails() {
    let (secret, public) = generate_keypair();
    let plaintext = b"guardian token to tamper";

    let mut sealed =
        sealed_box::seal(plaintext, &public).expect("seal succeeds for a valid recipient key");
    // Flip the last byte (inside the authentication tag)
    let last = sealed.last_mut().expect("sealed must be non-empty");
    *last ^= 0xFF;

    let result = sealed_box::open(&sealed, &secret);
    assert!(
        result.is_err(),
        "opening a tampered ciphertext must fail with authentication error"
    );
}

// @scenario: contact_recovery :: Sealed-box output size is ephemeral-pk plus nonce plus ciphertext plus tag
#[test]
fn test_sealed_box_output_size() {
    let (_secret, public) = generate_keypair();
    let plaintext = vec![0u8; 100];

    let sealed =
        sealed_box::seal(&plaintext, &public).expect("seal succeeds for a valid recipient key");

    // ephemeral_pk (32) + nonce (24) + ciphertext (100) + tag (16) = 172
    assert_eq!(
        sealed.len(),
        172,
        "sealed output must be exactly 32+24+100+16 = 172 bytes"
    );
}

// ── Mutation-coverage tests ─────────────────────────────────────

// @scenario: contact_recovery :: Output size scales linearly with plaintext
#[rstest]
#[case(0)]
#[case(1)]
#[case(31)]
#[case(32)]
#[case(255)]
#[case(1024)]
fn test_sealed_box_output_size_per_plaintext(#[case] plaintext_len: usize) {
    let (_secret, public) = generate_keypair();
    let plaintext = vec![0xAB; plaintext_len];

    let sealed =
        sealed_box::seal(&plaintext, &public).expect("seal succeeds for a valid recipient key");

    // Output = ephemeral_pk (32) + nonce (24) + ciphertext (plaintext_len) + tag (16)
    let expected = 32 + 24 + plaintext_len + 16;
    assert_eq!(sealed.len(), expected);
}

// @scenario: contact_recovery :: Open returns InvalidFormat for short input
#[rstest]
#[case(0)]
#[case(1)]
#[case(32)]
#[case(55)]
#[case(71)] // one byte short of MIN_SEALED_LEN
fn test_open_too_short_returns_invalid_format(#[case] len: usize) {
    let (secret, _public) = generate_keypair();
    let too_short = vec![0u8; len];
    let result = sealed_box::open(&too_short, &secret);
    // Pin the exact error variant. Catches mutations that swap which
    // error is returned (InvalidFormat ↔ DecryptionFailed).
    assert!(matches!(result, Err(RecoveryError::InvalidFormat)));
}

// @scenario: contact_recovery :: Open returns DecryptionFailed for wrong key
#[test]
fn test_open_with_wrong_key_returns_decryption_failed() {
    let (_secret_a, public_a) = generate_keypair();
    let (secret_b, _public_b) = generate_keypair();

    let plaintext = b"secret guardian token";
    let sealed =
        sealed_box::seal(plaintext, &public_a).expect("seal succeeds for a valid recipient key");

    let result = sealed_box::open(&sealed, &secret_b);
    // Pin the exact error variant — distinguishes a wrong-key failure
    // from a malformed-input failure.
    assert!(matches!(result, Err(RecoveryError::DecryptionFailed)));
}

// @scenario: contact_recovery :: Roundtrip across plaintext sizes
#[rstest]
#[case(b"".to_vec())] // empty
#[case(b"x".to_vec())] // single byte
#[case(vec![0u8; 16])] // tag-aligned
#[case(vec![0xFF; 64])] // larger
fn test_seal_open_roundtrip_various_sizes(#[case] plaintext: Vec<u8>) {
    let (secret, public) = generate_keypair();
    let sealed =
        sealed_box::seal(&plaintext, &public).expect("seal succeeds for a valid recipient key");
    let opened = sealed_box::open(&sealed, &secret).expect("open must succeed");
    assert_eq!(opened, plaintext);
}

// @scenario: contact_recovery :: Sender is anonymous (ephemeral key changes per call)
#[test]
fn test_seal_uses_fresh_ephemeral_key_each_call() {
    let (_secret, public) = generate_keypair();
    let plaintext = b"identical input";

    let a = sealed_box::seal(plaintext, &public).expect("seal succeeds for a valid recipient key");
    let b = sealed_box::seal(plaintext, &public).expect("seal succeeds for a valid recipient key");

    // The first 32 bytes are the ephemeral public key — must differ
    // each call. Catches mutations that fix the ephemeral keypair.
    assert_ne!(&a[..32], &b[..32], "ephemeral_pk must change per seal call");

    // Nonces (next 24 bytes) must also differ.
    assert_ne!(&a[32..56], &b[32..56], "nonce must be fresh per seal");

    // Ciphertext bytes also differ even for identical plaintext.
    assert_ne!(&a[56..], &b[56..], "ciphertext must differ");
}

// @scenario: contact_recovery :: Tampered ephemeral key fails authentication
#[test]
fn test_tampered_ephemeral_pk_fails_decryption() {
    let (secret, public) = generate_keypair();
    let plaintext = b"some payload";
    let mut sealed =
        sealed_box::seal(plaintext, &public).expect("seal succeeds for a valid recipient key");
    // Flip a bit in the ephemeral public key.
    sealed[0] ^= 0x01;

    let result = sealed_box::open(&sealed, &secret);
    // A modified ephemeral_pk derives a different shared secret →
    // different key → AEAD decryption fails.
    assert!(matches!(result, Err(RecoveryError::DecryptionFailed)));
}

// @scenario: contact_recovery :: Tampered nonce fails authentication
#[test]
fn test_tampered_nonce_fails_decryption() {
    let (secret, public) = generate_keypair();
    let plaintext = b"some payload";
    let mut sealed =
        sealed_box::seal(plaintext, &public).expect("seal succeeds for a valid recipient key");
    // Flip a bit inside the nonce region.
    sealed[40] ^= 0x80;

    let result = sealed_box::open(&sealed, &secret);
    assert!(matches!(result, Err(RecoveryError::DecryptionFailed)));
}

// @scenario: contact_recovery :: Truncated ciphertext fails authentication
#[test]
fn test_truncated_ciphertext_fails_decryption() {
    let (secret, public) = generate_keypair();
    let plaintext = b"plaintext that survives truncation";
    let mut sealed =
        sealed_box::seal(plaintext, &public).expect("seal succeeds for a valid recipient key");
    // Drop the last byte of the tag — remains >= MIN_SEALED_LEN if
    // plaintext is non-trivial.
    sealed.pop();

    let result = sealed_box::open(&sealed, &secret);
    assert!(matches!(result, Err(RecoveryError::DecryptionFailed)));
}

// @scenario: contact_recovery :: Exact MIN_SEALED_LEN with empty plaintext
#[test]
fn test_seal_open_empty_plaintext_at_min_length() {
    let (secret, public) = generate_keypair();
    let sealed = sealed_box::seal(b"", &public).expect("seal succeeds for a valid recipient key");
    // Empty plaintext means sealed is exactly 32 + 24 + 0 + 16 = 72 bytes
    // (MIN_SEALED_LEN). This tests the boundary of the length check.
    assert_eq!(sealed.len(), 72);
    let opened = sealed_box::open(&sealed, &secret).expect("must open");
    assert!(opened.is_empty());
}

// ── Adversarial: small-order recipient keys (CC-14) ─────────────
//
// The known small-order X25519 points (libsodium blacklist). DH against
// any of them collapses to the all-zero shared secret, so HKDF would
// produce a predictable key and seal() would emit a forgeable blob.
// seal() must reject them with WeakKey. Vodozemac-class — see problem
// record 2026-06-08-sealed-box-noncontributory-dh.
// @scenario: contact_recovery :: Seal rejects a small-order recipient key
#[rstest]
#[case("0000000000000000000000000000000000000000000000000000000000000000")]
#[case("0100000000000000000000000000000000000000000000000000000000000000")]
#[case("e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800")]
#[case("5f9c95bca3508c24b1d0b1559c83ef5b04445cc4581c8e86d8224eddd09f1157")]
#[case("ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f")]
#[case("edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f")]
#[case("eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f")]
fn test_seal_rejects_small_order_recipient(#[case] point_hex: &str) {
    let bytes: [u8; 32] = hex::decode(point_hex)
        .expect("valid hex vector")
        .try_into()
        .expect("32-byte point");
    let recipient = PublicKey::from(bytes);

    let result = sealed_box::seal(b"guardian token", &recipient);
    assert!(
        matches!(result, Err(RecoveryError::WeakKey)),
        "seal must reject small-order recipient {point_hex}: {result:?}"
    );
}
