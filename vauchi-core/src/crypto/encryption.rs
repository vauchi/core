// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Symmetric Encryption (XChaCha20-Poly1305 / AES-256-GCM)
//!
//! Provides authenticated encryption with a versioned ciphertext format.
//! New encryptions use XChaCha20-Poly1305 (spec-mandated). Legacy AES-256-GCM
//! data is still decryptable via algorithm tag dispatch.
//!
//! Ciphertext format: `algorithm_tag (1 byte) || nonce || ciphertext || tag`
//!   - Tag `0x01`: AES-256-GCM (12-byte nonce, 16-byte tag)
//!   - Tag `0x02`: XChaCha20-Poly1305 (24-byte nonce, 16-byte tag)
//!
//! Legacy (untagged) ciphertext: `nonce (12 bytes) || ciphertext || tag`
//! is auto-detected when the first byte is NOT a known algorithm tag.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::XChaCha20Poly1305;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use thiserror::Error;
use zeroize::Zeroize;

/// Encryption error types.
#[derive(Error, Debug)]
pub enum EncryptionError {
    #[error("Encryption failed")]
    EncryptionFailed,
    #[error("Decryption failed: data may be corrupted or wrong key")]
    DecryptionFailed,
    #[error("Ciphertext too short")]
    CiphertextTooShort,
}

/// Algorithm tag for AES-256-GCM.
const ALG_TAG_AES_GCM: u8 = 0x01;
/// Algorithm tag for XChaCha20-Poly1305.
const ALG_TAG_XCHACHA20: u8 = 0x02;

/// Nonce size for AES-256-GCM (96 bits = 12 bytes).
const AES_GCM_NONCE_SIZE: usize = 12;
/// Nonce size for XChaCha20-Poly1305 (192 bits = 24 bytes).
const XCHACHA20_NONCE_SIZE: usize = 24;
/// Authentication tag size (16 bytes for both algorithms).
const TAG_SIZE: usize = 16;

/// 256-bit symmetric encryption key.
#[derive(Clone)]
pub struct SymmetricKey {
    bytes: [u8; 32],
}

impl std::fmt::Debug for SymmetricKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Don't expose key bytes in debug output
        f.debug_struct("SymmetricKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl Drop for SymmetricKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl SymmetricKey {
    /// Generates a new random symmetric key.
    pub fn generate() -> Self {
        let rng = SystemRandom::new();
        let key = ring::rand::generate::<[u8; 32]>(&rng)
            .expect("System RNG should not fail")
            .expose();
        SymmetricKey { bytes: key }
    }

    /// Creates a key from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        SymmetricKey { bytes }
    }

    /// Returns a reference to the key bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

/// Encrypts data using XChaCha20-Poly1305 (default algorithm).
///
/// Output format: `0x02 || nonce (24 bytes) || ciphertext || tag (16 bytes)`
pub fn encrypt(key: &SymmetricKey, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    let rng = SystemRandom::new();

    // Generate random 24-byte nonce
    let mut nonce_bytes = [0u8; XCHACHA20_NONCE_SIZE];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    // Tagged format: algorithm_tag || nonce || ciphertext+tag
    let mut output = Vec::with_capacity(1 + XCHACHA20_NONCE_SIZE + ciphertext.len());
    output.push(ALG_TAG_XCHACHA20);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    Ok(output)
}

/// Decrypts data, auto-detecting the algorithm from the ciphertext format.
///
/// Supports:
/// - Tagged XChaCha20-Poly1305 (tag `0x02`)
/// - Tagged AES-256-GCM (tag `0x01`)
/// - Legacy untagged AES-256-GCM (12-byte nonce prefix)
pub fn decrypt(key: &SymmetricKey, ciphertext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if ciphertext.is_empty() {
        return Err(EncryptionError::CiphertextTooShort);
    }

    match ciphertext[0] {
        ALG_TAG_XCHACHA20 => decrypt_xchacha20(key, &ciphertext[1..]),
        ALG_TAG_AES_GCM => decrypt_aes_gcm(key, &ciphertext[1..]),
        _ => {
            // Legacy untagged AES-256-GCM: nonce (12) || ciphertext || tag (16)
            decrypt_aes_gcm(key, ciphertext)
        }
    }
}

/// Decrypts XChaCha20-Poly1305 data.
///
/// Input format: `nonce (24 bytes) || ciphertext || tag (16 bytes)`
fn decrypt_xchacha20(key: &SymmetricKey, data: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    let min_size = XCHACHA20_NONCE_SIZE + TAG_SIZE;
    if data.len() < min_size {
        return Err(EncryptionError::CiphertextTooShort);
    }

    let nonce = chacha20poly1305::XNonce::from_slice(&data[..XCHACHA20_NONCE_SIZE]);
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());

    cipher
        .decrypt(nonce, &data[XCHACHA20_NONCE_SIZE..])
        .map_err(|_| EncryptionError::DecryptionFailed)
}

/// Decrypts AES-256-GCM data.
///
/// Input format: `nonce (12 bytes) || ciphertext || tag (16 bytes)`
fn decrypt_aes_gcm(key: &SymmetricKey, data: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    let min_size = AES_GCM_NONCE_SIZE + AES_256_GCM.tag_len();
    if data.len() < min_size {
        return Err(EncryptionError::CiphertextTooShort);
    }

    let nonce_bytes: [u8; AES_GCM_NONCE_SIZE] = data[..AES_GCM_NONCE_SIZE]
        .try_into()
        .map_err(|_| EncryptionError::DecryptionFailed)?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let unbound_key = UnboundKey::new(&AES_256_GCM, key.as_bytes())
        .map_err(|_| EncryptionError::DecryptionFailed)?;
    let opening_key = LessSafeKey::new(unbound_key);

    let mut buffer = data[AES_GCM_NONCE_SIZE..].to_vec();
    let plaintext = opening_key
        .open_in_place(nonce, Aad::empty(), &mut buffer)
        .map_err(|_| EncryptionError::DecryptionFailed)?;

    Ok(plaintext.to_vec())
}

/// Encrypts data using AES-256-GCM with algorithm tag.
///
/// Output format: `0x01 || nonce (12 bytes) || ciphertext || tag (16 bytes)`
///
/// Primarily for testing backward compatibility. New code should use `encrypt()`.
pub fn encrypt_aes_gcm(key: &SymmetricKey, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    let rng = SystemRandom::new();

    let mut nonce_bytes = [0u8; AES_GCM_NONCE_SIZE];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let unbound_key = UnboundKey::new(&AES_256_GCM, key.as_bytes())
        .map_err(|_| EncryptionError::EncryptionFailed)?;
    let sealing_key = LessSafeKey::new(unbound_key);

    let mut in_out = plaintext.to_vec();
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    sealing_key
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let mut output = Vec::with_capacity(1 + AES_GCM_NONCE_SIZE + in_out.len());
    output.push(ALG_TAG_AES_GCM);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&in_out);

    Ok(output)
}

/// Encrypts data using legacy untagged AES-256-GCM format.
///
/// Output format: `nonce (12 bytes) || ciphertext || tag (16 bytes)`
///
/// Only for testing migration from pre-tag format. Never use in production.
pub fn encrypt_legacy_untagged(
    key: &SymmetricKey,
    plaintext: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    let rng = SystemRandom::new();

    let mut nonce_bytes = [0u8; AES_GCM_NONCE_SIZE];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let unbound_key = UnboundKey::new(&AES_256_GCM, key.as_bytes())
        .map_err(|_| EncryptionError::EncryptionFailed)?;
    let sealing_key = LessSafeKey::new(unbound_key);

    let mut in_out = plaintext.to_vec();
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    sealing_key
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let mut output = Vec::with_capacity(AES_GCM_NONCE_SIZE + in_out.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&in_out);

    Ok(output)
}
