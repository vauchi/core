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
pub fn seal(plaintext: &[u8], recipient_pk: &PublicKey) -> Vec<u8> {
    let ephemeral_secret = StaticSecret::random_from_rng(OsRng);
    let ephemeral_pk = PublicKey::from(&ephemeral_secret);

    // 2. DH: ephemeral_secret × recipient_pk → shared secret.
    let shared = ephemeral_secret.diffie_hellman(recipient_pk);

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
    out
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

    // 3. Derive symmetric key via HKDF-SHA256 (ADR-007).
    let key = HKDF::derive_key(None, shared.as_bytes(), DOMAIN);

    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .expect("32-byte key is always valid for XChaCha20Poly1305");
    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| RecoveryError::DecryptionFailed)
}
