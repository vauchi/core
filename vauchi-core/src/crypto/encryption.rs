// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Symmetric Encryption (XChaCha20-Poly1305)
//!
//! Provides authenticated encryption with a versioned ciphertext format.
//! All encryptions use XChaCha20-Poly1305 (spec-mandated).
//!
//! Ciphertext format: `algorithm_tag (1 byte) || nonce || ciphertext || tag`
//!   - Tag `0x02`: XChaCha20-Poly1305 (24-byte nonce, 16-byte tag)
//!   - Tag `0x03`: XChaCha20-Poly1305 with associated data (24-byte nonce, 16-byte tag)

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
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
    #[error("Degenerate key: all-zeros key rejected")]
    DegenerateKey,
}

/// Algorithm tag for XChaCha20-Poly1305.
const ALG_TAG_XCHACHA20: u8 = 0x02;
/// Algorithm tag for XChaCha20-Poly1305 with associated data binding.
const ALG_TAG_XCHACHA20_AD: u8 = 0x03;

/// Nonce size for XChaCha20-Poly1305 (192 bits = 24 bytes).
const XCHACHA20_NONCE_SIZE: usize = 24;
/// Authentication tag size (16 bytes).
const TAG_SIZE: usize = 16;

/// 256-bit symmetric encryption key.
///
/// Security properties:
/// - `Clone`: Safe — both original and clone are zeroized on drop via `ZeroizeOnDrop`.
/// - `Debug`: Redacted — key bytes never appear in debug/log output.
/// - `Drop`: Automatic zeroization via `ZeroizeOnDrop` derive.
#[derive(Clone, Zeroize, zeroize::ZeroizeOnDrop)]
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

impl SymmetricKey {
    /// Generates a new random symmetric key.
    pub fn generate() -> Self {
        let key: [u8; 32] = super::random_bytes();
        SymmetricKey { bytes: key }
    }

    /// Creates a key from raw bytes.
    ///
    /// # Panics
    ///
    /// Panics if the key is all zeros (degenerate key).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        assert!(
            bytes.iter().any(|&b| b != 0),
            "SymmetricKey::from_bytes: all-zeros key is degenerate and rejected"
        );
        SymmetricKey { bytes }
    }

    /// Creates a key from raw bytes, returning an error for degenerate (all-zeros) keys.
    ///
    /// Use this at trust boundaries (deserialization, network input) where
    /// panicking is unacceptable.
    pub fn try_from_bytes(mut bytes: [u8; 32]) -> Result<Self, EncryptionError> {
        if bytes.iter().all(|&b| b == 0) {
            bytes.zeroize();
            return Err(EncryptionError::DegenerateKey);
        }
        Ok(SymmetricKey { bytes })
    }

    /// Creates a key from raw bytes without validation.
    ///
    /// Only use this for deserialization of keys already validated and stored
    /// (e.g., from encrypted storage). At trust boundaries (network input,
    /// user-supplied data), prefer [`try_from_bytes`](Self::try_from_bytes)
    /// which returns `Result` instead of panicking.
    pub fn from_bytes_unchecked(bytes: [u8; 32]) -> Self {
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
///
/// # Nonce Security (Tracker #226)
///
/// Each encryption generates a fresh 24-byte (192-bit) nonce from
/// `rand::rngs::OsRng` (OS CSPRNG). The 192-bit nonce space of
/// XChaCha20-Poly1305 makes random collision negligible even at high
/// volume (~2^96 encryptions before birthday-bound concern). This is
/// why XChaCha20 was chosen over AES-GCM (96-bit nonce, birthday-bound
/// at ~2^32 encryptions per key).
pub fn encrypt(key: &SymmetricKey, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    // Generate cryptographically random 24-byte nonce from OS CSPRNG
    let nonce_bytes: [u8; XCHACHA20_NONCE_SIZE] = super::random_bytes();

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

/// Encrypts data using XChaCha20-Poly1305 with associated data binding.
///
/// The associated data (AD) is authenticated but not included in the output.
/// Both parties must use the same AD for decryption to succeed. This binds
/// the ciphertext to its context (e.g., message header fields), preventing
/// header manipulation and message reuse attacks.
///
/// Output format: `0x03 || nonce (24 bytes) || ciphertext || tag (16 bytes)`
pub fn encrypt_with_ad(
    key: &SymmetricKey,
    plaintext: &[u8],
    ad: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    let nonce_bytes: [u8; XCHACHA20_NONCE_SIZE] = super::random_bytes();

    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);

    let payload = Payload {
        msg: plaintext,
        aad: ad,
    };
    let ciphertext = cipher
        .encrypt(nonce, payload)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let mut output = Vec::with_capacity(1 + XCHACHA20_NONCE_SIZE + ciphertext.len());
    output.push(ALG_TAG_XCHACHA20_AD);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    Ok(output)
}

/// Decrypts data, auto-detecting the algorithm from the ciphertext format.
///
/// Supports:
/// - Tagged XChaCha20-Poly1305 (tag `0x02`)
///
/// Note: Cannot decrypt tag `0x03` (AD-bound) — use `decrypt_with_ad` instead.
pub fn decrypt(key: &SymmetricKey, ciphertext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if ciphertext.is_empty() {
        return Err(EncryptionError::CiphertextTooShort);
    }

    match ciphertext[0] {
        ALG_TAG_XCHACHA20_AD => Err(EncryptionError::DecryptionFailed), // Requires AD
        ALG_TAG_XCHACHA20 => decrypt_xchacha20(key, &ciphertext[1..]),
        _ => Err(EncryptionError::DecryptionFailed),
    }
}

/// Decrypts data with associated data, auto-detecting the algorithm.
///
/// For tag `0x03` (AD-bound), the provided AD is used for authentication.
/// For tag `0x02`, AD is ignored (backward compatibility with non-AD ciphertext).
pub fn decrypt_with_ad(
    key: &SymmetricKey,
    ciphertext: &[u8],
    ad: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    if ciphertext.is_empty() {
        return Err(EncryptionError::CiphertextTooShort);
    }

    match ciphertext[0] {
        ALG_TAG_XCHACHA20_AD => decrypt_xchacha20_ad(key, &ciphertext[1..], ad),
        ALG_TAG_XCHACHA20 => decrypt_xchacha20(key, &ciphertext[1..]),
        _ => Err(EncryptionError::DecryptionFailed),
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

/// Decrypts XChaCha20-Poly1305 data with associated data.
///
/// Input format: `nonce (24 bytes) || ciphertext || tag (16 bytes)`
fn decrypt_xchacha20_ad(
    key: &SymmetricKey,
    data: &[u8],
    ad: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    let min_size = XCHACHA20_NONCE_SIZE + TAG_SIZE;
    if data.len() < min_size {
        return Err(EncryptionError::CiphertextTooShort);
    }

    let nonce = chacha20poly1305::XNonce::from_slice(&data[..XCHACHA20_NONCE_SIZE]);
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());

    let payload = Payload {
        msg: &data[XCHACHA20_NONCE_SIZE..],
        aad: ad,
    };

    cipher
        .decrypt(nonce, payload)
        .map_err(|_| EncryptionError::DecryptionFailed)
}
