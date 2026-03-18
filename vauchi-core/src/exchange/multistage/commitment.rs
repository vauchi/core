// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commitment scheme for the multi-stage atomic QR exchange protocol.
//!
//! Creates an encrypted payload (ChaCha20-Poly1305 with a random reveal key) and a
//! binding hash (SHA-256(reveal_key || ciphertext [|| context])). The reveal key is
//! withheld until Stage 3 (VERIFY), ensuring neither side can decrypt until both
//! parties exchange reveal keys.
//!
//! The optional context parameter (T1.7) binds relay metadata (URL + Noise pubkey)
//! into the commitment hash, preventing a MitM from swapping relay fields in the
//! INIT QR without invalidating the commitment.

use aws_lc_rs::digest::{digest, SHA256};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::ChaCha20Poly1305;
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::Zeroize;

/// Errors that can occur during commitment operations.
#[derive(Debug, Error)]
pub enum CommitmentError {
    /// Encryption failed (RNG or AEAD error).
    #[error("encryption failed")]
    EncryptionFailed,
    /// Decryption failed — wrong key or tampered ciphertext.
    #[error("decryption failed — wrong key or tampered ciphertext")]
    DecryptionFailed,
}

/// A commitment: ciphertext + hash, with reveal key held separately.
///
/// The reveal key is zeroized on drop to prevent leaking key material.
pub struct Commitment {
    reveal_key: [u8; 32],
    ciphertext: Vec<u8>, // nonce (12 bytes) || encrypted || tag (16 bytes)
    hash: [u8; 32],      // SHA256(reveal_key || ciphertext [|| context])
}

impl Commitment {
    /// Create a new commitment for the given plaintext (no context binding).
    ///
    /// Generates a random reveal key and encrypts with ChaCha20-Poly1305.
    /// The commitment hash binds the reveal key to the ciphertext.
    #[allow(dead_code)] // used by external tests
    pub fn create(plaintext: &[u8]) -> Self {
        Self::create_with_context(plaintext, b"")
    }

    /// Create a new commitment with context binding (T1.7).
    ///
    /// The context (e.g., relay URL + Noise pubkey) is included in the
    /// commitment hash but NOT encrypted. This prevents a MitM from
    /// swapping context fields without invalidating the commitment.
    pub fn create_with_context(plaintext: &[u8], context: &[u8]) -> Self {
        let reveal_key: [u8; 32] = crate::crypto::random_bytes();

        let ciphertext = Self::encrypt(&reveal_key, plaintext).expect("encryption failed");
        let hash = Self::compute_hash_with_context(&reveal_key, &ciphertext, context);

        Commitment {
            reveal_key,
            ciphertext,
            hash,
        }
    }

    /// Returns the reveal key (to be sent in Stage 3 VERIFY).
    pub fn reveal_key(&self) -> &[u8; 32] {
        &self.reveal_key
    }

    /// Returns the ciphertext (nonce || encrypted || tag).
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    /// Returns the binding hash: SHA-256(reveal_key || ciphertext).
    pub fn hash(&self) -> &[u8; 32] {
        &self.hash
    }

    /// Decrypt using the held reveal key.
    #[allow(dead_code)]
    pub fn open(&self) -> Result<Vec<u8>, CommitmentError> {
        Self::open_with_key(&self.reveal_key, &self.ciphertext)
    }

    /// Decrypt with an externally provided reveal key.
    pub fn open_with_key(
        reveal_key: &[u8; 32],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CommitmentError> {
        Self::decrypt(reveal_key, ciphertext)
    }

    /// Verify that a hash matches SHA-256(reveal_key || ciphertext) (no context).
    ///
    /// Uses constant-time comparison to prevent timing side-channels.
    #[allow(dead_code)] // used by external tests
    pub fn verify_hash(reveal_key: &[u8; 32], ciphertext: &[u8], expected_hash: &[u8; 32]) -> bool {
        Self::verify_hash_with_context(reveal_key, ciphertext, expected_hash, b"")
    }

    /// Verify that a hash matches SHA-256(reveal_key || ciphertext || context).
    ///
    /// The context must match exactly what was passed to `create_with_context`.
    /// Uses constant-time comparison to prevent timing side-channels.
    pub fn verify_hash_with_context(
        reveal_key: &[u8; 32],
        ciphertext: &[u8],
        expected_hash: &[u8; 32],
        context: &[u8],
    ) -> bool {
        let computed = Self::compute_hash_with_context(reveal_key, ciphertext, context);
        computed.ct_eq(expected_hash).into()
    }

    fn compute_hash_with_context(
        reveal_key: &[u8; 32],
        ciphertext: &[u8],
        context: &[u8],
    ) -> [u8; 32] {
        let mut input = Vec::with_capacity(32 + ciphertext.len() + context.len());
        input.extend_from_slice(reveal_key);
        input.extend_from_slice(ciphertext);
        input.extend_from_slice(context);
        let d = digest(&SHA256, &input);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(d.as_ref());
        input.zeroize();
        hash
    }

    fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CommitmentError> {
        let nonce_bytes: [u8; 12] = crate::crypto::random_bytes();

        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| CommitmentError::EncryptionFailed)?;

        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    fn decrypt(key: &[u8; 32], ciphertext: &[u8]) -> Result<Vec<u8>, CommitmentError> {
        if ciphertext.len() < 12 + 16 {
            return Err(CommitmentError::DecryptionFailed);
        }
        let (nonce_bytes, encrypted) = ciphertext.split_at(12);

        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);

        cipher
            .decrypt(nonce, encrypted)
            .map_err(|_| CommitmentError::DecryptionFailed)
    }
}

impl Drop for Commitment {
    fn drop(&mut self) {
        self.reveal_key.zeroize();
    }
}
