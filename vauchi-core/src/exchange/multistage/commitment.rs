// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commitment scheme for the multi-stage atomic QR exchange protocol.
//!
//! Creates an encrypted payload (ChaCha20-Poly1305 with a random reveal key) and a
//! binding hash (SHA-256(reveal_key || ciphertext)). The reveal key is withheld
//! until Stage 3 (VERIFY), ensuring neither side can decrypt until both parties
//! exchange reveal keys.

use aws_lc_rs::digest::{digest, SHA256};
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
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
    hash: [u8; 32],      // SHA256(reveal_key || ciphertext)
}

impl Commitment {
    /// Create a new commitment for the given plaintext.
    ///
    /// Generates a random reveal key and encrypts with ChaCha20-Poly1305.
    /// The commitment hash binds the reveal key to the ciphertext.
    pub fn create(plaintext: &[u8]) -> Self {
        let rng = SystemRandom::new();
        let mut reveal_key = [0u8; 32];
        rng.fill(&mut reveal_key).expect("RNG failed");

        let ciphertext = Self::encrypt(&reveal_key, plaintext).expect("encryption failed");
        let hash = Self::compute_hash(&reveal_key, &ciphertext);

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

    /// Verify that a hash matches SHA-256(reveal_key || ciphertext).
    ///
    /// Uses constant-time comparison to prevent timing side-channels.
    pub fn verify_hash(reveal_key: &[u8; 32], ciphertext: &[u8], expected_hash: &[u8; 32]) -> bool {
        let computed = Self::compute_hash(reveal_key, ciphertext);
        computed.ct_eq(expected_hash).into()
    }

    fn compute_hash(reveal_key: &[u8; 32], ciphertext: &[u8]) -> [u8; 32] {
        let mut input = Vec::with_capacity(32 + ciphertext.len());
        input.extend_from_slice(reveal_key);
        input.extend_from_slice(ciphertext);
        let d = digest(&SHA256, &input);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(d.as_ref());
        input.zeroize();
        hash
    }

    fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CommitmentError> {
        let rng = SystemRandom::new();
        let mut nonce_bytes = [0u8; 12];
        rng.fill(&mut nonce_bytes)
            .map_err(|_| CommitmentError::EncryptionFailed)?;

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
