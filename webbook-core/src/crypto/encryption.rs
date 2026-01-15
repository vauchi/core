//! Symmetric Encryption (AES-256-GCM)
//!
//! Provides authenticated encryption using AES-256-GCM via the audited `ring` library.
//! Each encryption uses a random 96-bit nonce prepended to the ciphertext.

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

/// Nonce size for AES-256-GCM (96 bits = 12 bytes).
const NONCE_SIZE: usize = 12;

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

/// Encrypts data with a symmetric key using AES-256-GCM.
///
/// The output format is: `nonce (12 bytes) || ciphertext || tag (16 bytes)`
///
/// A random nonce is generated for each encryption to ensure semantic security.
pub fn encrypt(key: &SymmetricKey, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    let rng = SystemRandom::new();

    // Generate random nonce
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    // Create AEAD key
    let unbound_key = UnboundKey::new(&AES_256_GCM, key.as_bytes())
        .map_err(|_| EncryptionError::EncryptionFailed)?;
    let sealing_key = LessSafeKey::new(unbound_key);

    // Prepare buffer for encryption (just the plaintext, tag will be appended)
    let mut in_out = plaintext.to_vec();

    // Encrypt in place and append tag
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    sealing_key
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    // Prepend nonce to the ciphertext
    let mut output = Vec::with_capacity(NONCE_SIZE + in_out.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&in_out);

    Ok(output)
}

/// Decrypts data with a symmetric key using AES-256-GCM.
///
/// Expects input format: `nonce (12 bytes) || ciphertext || tag (16 bytes)`
pub fn decrypt(key: &SymmetricKey, ciphertext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    // Minimum size: nonce + tag (no plaintext is valid)
    let min_size = NONCE_SIZE + AES_256_GCM.tag_len();
    if ciphertext.len() < min_size {
        return Err(EncryptionError::CiphertextTooShort);
    }

    // Extract nonce from the beginning
    let nonce_bytes: [u8; NONCE_SIZE] = ciphertext[..NONCE_SIZE]
        .try_into()
        .map_err(|_| EncryptionError::DecryptionFailed)?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    // Create AEAD key
    let unbound_key = UnboundKey::new(&AES_256_GCM, key.as_bytes())
        .map_err(|_| EncryptionError::DecryptionFailed)?;
    let opening_key = LessSafeKey::new(unbound_key);

    // Copy ciphertext (after nonce) to mutable buffer for in-place decryption
    let mut buffer = ciphertext[NONCE_SIZE..].to_vec();

    // Decrypt in place
    let plaintext = opening_key
        .open_in_place(nonce, Aad::empty(), &mut buffer)
        .map_err(|_| EncryptionError::DecryptionFailed)?;

    Ok(plaintext.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_roundtrip() {
        let key = SymmetricKey::generate();
        let data = b"test data";
        let encrypted = encrypt(&key, data).unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(data.to_vec(), decrypted);
    }

    #[test]
    fn test_empty_data() {
        let key = SymmetricKey::generate();
        let data = b"";
        let encrypted = encrypt(&key, data).unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(data.to_vec(), decrypted);
    }
}
