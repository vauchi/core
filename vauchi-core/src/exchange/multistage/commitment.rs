// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commitment scheme for the multi-stage atomic QR exchange protocol.
//!
//! Creates an encrypted payload (AES-256-GCM with a random reveal key) and a
//! binding hash (SHA-256(reveal_key || ciphertext)). The reveal key is withheld
//! until Stage 3 (VERIFY), ensuring neither side can decrypt until both parties
//! exchange reveal keys.

use aws_lc_rs::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use aws_lc_rs::digest::{digest, SHA256};
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
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
    /// Generates a random reveal key and encrypts with AES-256-GCM.
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

        let unbound =
            UnboundKey::new(&AES_256_GCM, key).map_err(|_| CommitmentError::EncryptionFailed)?;
        let sealing_key = LessSafeKey::new(unbound);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut in_out = plaintext.to_vec();
        sealing_key
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| CommitmentError::EncryptionFailed)?;

        let mut result = Vec::with_capacity(12 + in_out.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&in_out);
        Ok(result)
    }

    fn decrypt(key: &[u8; 32], ciphertext: &[u8]) -> Result<Vec<u8>, CommitmentError> {
        if ciphertext.len() < 12 + 16 {
            return Err(CommitmentError::DecryptionFailed);
        }
        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(nonce_bytes);

        let unbound =
            UnboundKey::new(&AES_256_GCM, key).map_err(|_| CommitmentError::DecryptionFailed)?;
        let opening_key = LessSafeKey::new(unbound);
        let nonce = Nonce::assume_unique_for_key(nonce_arr);

        let mut in_out = encrypted.to_vec();
        let plaintext = opening_key
            .open_in_place(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| CommitmentError::DecryptionFailed)?;
        Ok(plaintext.to_vec())
    }
}

impl Drop for Commitment {
    fn drop(&mut self) {
        self.reveal_key.zeroize();
    }
}
