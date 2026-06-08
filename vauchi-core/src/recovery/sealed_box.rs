// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sealed-box encryption for guardian token relay storage.
//!
//! Implements anonymous sender encryption using ephemeral X25519 key agreement
//! and XChaCha20-Poly1305 AEAD. Only the designated recipient (guardian) can
//! decrypt their entry. The sender is anonymous — no long-term sender key is
//! included in the output.
//!
//! # Output format
//!
//! `ephemeral_pk (32) || nonce (24) || ciphertext+tag`
//!
//! Minimum sealed size for `open`: 32 + 24 + 16 = 72 bytes.
//!
//! # Key derivation
//!
//! HKDF-SHA256(salt=None, ikm=shared_secret, info="vauchi-sealed-box-v1") → 32-byte symmetric key.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use rand::rngs::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};

use super::RecoveryError;
use crate::crypto::HKDF;

/// Domain separation string for key derivation.
const DOMAIN: &[u8] = b"vauchi-sealed-box-v1";

/// Minimum size of a valid sealed blob: ephemeral_pk (32) + nonce (24) + tag (16).
const MIN_SEALED_LEN: usize = 32 + 24 + 16;

/// Seal `plaintext` so only the holder of `recipient_pk`'s secret key can open it.
///
/// Uses an ephemeral X25519 keypair for the DH exchange; the sender is anonymous.
///
/// # Output format
///
/// `ephemeral_pk (32) || nonce (24) || ciphertext+tag`
///
/// # Errors
///
/// Returns [`RecoveryError::WeakKey`] if `recipient_pk` is a small-order
/// point, which would collapse the DH output to an all-zero (predictable)
/// shared secret.
pub fn seal(plaintext: &[u8], recipient_pk: &PublicKey) -> Result<Vec<u8>, RecoveryError> {
    let ephemeral_secret = StaticSecret::random_from_rng(OsRng);
    let ephemeral_pk = PublicKey::from(&ephemeral_secret);

    // 2. DH: ephemeral_secret × recipient_pk → shared secret.
    let shared = ephemeral_secret.diffie_hellman(recipient_pk);
    // Reject small-order recipient keys: a non-contributory DH collapses
    // the shared secret to all-zeros, making the HKDF key predictable.
    if !shared.was_contributory() {
        return Err(RecoveryError::WeakKey);
    }

    // 3. Derive symmetric key via HKDF-SHA256 (ADR-007).
    let key = HKDF::derive_key(None, shared.as_bytes(), DOMAIN);

    let mut nonce_bytes = [0u8; 24];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from(nonce_bytes);

    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .expect("32-byte key is always valid for XChaCha20Poly1305");
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .expect("XChaCha20Poly1305 encryption is infallible for valid key/nonce");

    // 6. Assemble output: ephemeral_pk || nonce || ciphertext+tag.
    let mut out = Vec::with_capacity(32 + 24 + ciphertext.len());
    out.extend_from_slice(ephemeral_pk.as_bytes());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Open a sealed blob using the recipient's `StaticSecret`.
///
/// # Errors
///
/// Returns [`RecoveryError::InvalidFormat`] if the blob is too short.
/// Returns [`RecoveryError::DecryptionFailed`] if authentication fails
/// (wrong key, tampered ciphertext, or truncated input).
pub fn open(sealed: &[u8], recipient_secret: &StaticSecret) -> Result<Vec<u8>, RecoveryError> {
    if sealed.len() < MIN_SEALED_LEN {
        return Err(RecoveryError::InvalidFormat);
    }

    let ephemeral_pk_bytes: [u8; 32] = sealed[..32].try_into().expect("slice length checked above");
    let ephemeral_pk = PublicKey::from(ephemeral_pk_bytes);

    let nonce_bytes: [u8; 24] = sealed[32..56]
        .try_into()
        .expect("slice length checked above");
    let nonce = XNonce::from(nonce_bytes);

    let ciphertext = &sealed[56..];

    // 2. DH: recipient_secret × ephemeral_pk → shared secret.
    let shared = recipient_secret.diffie_hellman(&ephemeral_pk);
    // Reject non-contributory DH: an attacker-supplied small-order
    // ephemeral point collapses the shared secret to all-zeros, yielding
    // a predictable key and a forgeable blob (2026-06-08 audit).
    if !shared.was_contributory() {
        return Err(RecoveryError::DecryptionFailed);
    }

    // 3. Derive symmetric key via HKDF-SHA256 (ADR-007).
    let key = HKDF::derive_key(None, shared.as_bytes(), DOMAIN);

    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .expect("32-byte key is always valid for XChaCha20Poly1305");
    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| RecoveryError::DecryptionFailed)
}

// INLINE_TEST_REQUIRED: forges a blob using the private DOMAIN constant to
// exercise the open()-side small-order-point rejection
#[cfg(test)]
mod tests {
    use super::*;

    // An attacker who never knew the recipient's secret can still forge a
    // sealed blob by choosing a small-order ephemeral point: DH against it
    // collapses to the all-zero shared secret, so the HKDF key is
    // predictable and the attacker can seal chosen plaintext under it.
    // open() must reject the non-contributory DH before it is tricked into
    // returning the forgery (problem record
    // 2026-06-08-sealed-box-noncontributory-dh).
    #[test]
    fn open_rejects_forged_small_order_ephemeral() {
        let recipient_secret = StaticSecret::random_from_rng(OsRng);

        let predictable_key = HKDF::derive_key(None, &[0u8; 32], DOMAIN);
        let nonce_bytes = [7u8; 24];
        let cipher =
            XChaCha20Poly1305::new_from_slice(predictable_key.as_ref()).expect("32-byte key");
        let forged_ct = cipher
            .encrypt(
                &XNonce::from(nonce_bytes),
                b"attacker-chosen token".as_ref(),
            )
            .expect("encrypt under predictable key");

        let mut blob = Vec::new();
        blob.extend_from_slice(&[0u8; 32]); // small-order ephemeral_pk
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&forged_ct);

        let result = open(&blob, &recipient_secret);
        assert!(
            matches!(result, Err(RecoveryError::DecryptionFailed)),
            "open must reject a non-contributory ephemeral, not return the forgery: {result:?}"
        );
    }
}
