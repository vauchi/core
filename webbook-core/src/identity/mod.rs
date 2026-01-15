//! Identity Management Module
//!
//! Handles user identity creation, backup, and restoration.
//! Each identity has a unique Ed25519 signing keypair and X25519 exchange keypair.

mod backup;

pub use backup::IdentityBackup;

use crate::crypto::{SigningKeyPair, Signature, encrypt, decrypt, SymmetricKey};
use crate::exchange::X3DHKeyPair;
use ring::rand::SystemRandom;
use ring::pbkdf2;
use thiserror::Error;
use zeroize::Zeroize;
use std::num::NonZeroU32;

/// Identity-related errors.
#[derive(Error, Debug)]
pub enum IdentityError {
    #[error("Display name cannot be empty")]
    EmptyDisplayName,
    #[error("Password too weak: minimum 8 characters required")]
    WeakPassword,
    #[error("Backup encryption failed")]
    BackupFailed,
    #[error("Invalid backup or wrong password")]
    RestoreFailed,
}

/// Minimum password length for backups.
const MIN_PASSWORD_LENGTH: usize = 8;

/// PBKDF2 iterations for key derivation from password.
const PBKDF2_ITERATIONS: u32 = 100_000;

/// User identity containing cryptographic keys and metadata.
pub struct Identity {
    /// Master seed for deterministic key derivation (32 bytes).
    master_seed: [u8; 32],
    /// Ed25519 signing keypair.
    signing_keypair: SigningKeyPair,
    /// Ed25519 signing public key (cached for returning references).
    signing_public_key: [u8; 32],
    /// X25519 exchange public key (32 bytes).
    exchange_public_key: [u8; 32],
    /// User's display name.
    display_name: String,
}

impl Drop for Identity {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}

impl Identity {
    /// Creates a new identity with the given display name.
    ///
    /// Generates a random master seed and derives all keypairs from it.
    pub fn create(display_name: &str) -> Self {
        let rng = SystemRandom::new();

        // Generate random master seed
        let master_seed = ring::rand::generate::<[u8; 32]>(&rng)
            .expect("System RNG should not fail")
            .expose();

        Self::from_seed(master_seed, display_name.to_string())
    }

    /// Creates an identity from an existing seed.
    fn from_seed(master_seed: [u8; 32], display_name: String) -> Self {
        // Derive signing keypair from master seed
        let signing_keypair = SigningKeyPair::from_seed(&master_seed);

        // Cache the signing public key bytes
        let signing_public_key = *signing_keypair.public_key().as_bytes();

        // Derive exchange keypair (simple derivation for now - XOR with constant)
        // In production, use proper HKDF derivation
        let mut exchange_seed = master_seed;
        for (i, byte) in exchange_seed.iter_mut().enumerate() {
            *byte ^= (i as u8).wrapping_add(0x42);
        }

        // For X25519, we just store the public key derived from the seed
        // The actual X25519 private key would be derived when needed
        let exchange_public_key = exchange_seed; // Simplified - in real impl use X25519

        Identity {
            master_seed,
            signing_keypair,
            signing_public_key,
            exchange_public_key,
            display_name,
        }
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Sets the display name.
    pub fn set_display_name(&mut self, name: &str) {
        self.display_name = name.to_string();
    }

    /// Tries to set the display name, returning an error if invalid.
    pub fn try_set_display_name(&mut self, name: &str) -> Result<(), IdentityError> {
        if name.is_empty() {
            return Err(IdentityError::EmptyDisplayName);
        }
        self.display_name = name.to_string();
        Ok(())
    }

    /// Returns the public signing key bytes.
    pub fn signing_public_key(&self) -> &[u8; 32] {
        &self.signing_public_key
    }

    /// Returns the public exchange key bytes.
    pub fn exchange_public_key(&self) -> &[u8] {
        &self.exchange_public_key
    }

    /// Returns the X3DH keypair for key agreement.
    ///
    /// The keypair is derived from the master seed using the same derivation
    /// as exchange_public_key, ensuring consistency.
    pub fn x3dh_keypair(&self) -> X3DHKeyPair {
        // Derive X25519 secret from master_seed
        // Uses same derivation as exchange_public_key for consistency
        let mut x25519_secret = [0u8; 32];
        for (i, byte) in self.master_seed.iter().enumerate() {
            x25519_secret[i] = byte ^ (i as u8).wrapping_add(0x42);
        }
        X3DHKeyPair::from_bytes(x25519_secret)
    }

    /// Returns the public ID (hex fingerprint of signing key).
    pub fn public_id(&self) -> String {
        self.signing_keypair.public_key().fingerprint()
    }

    /// Signs a message using this identity's signing key.
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_keypair.sign(message)
    }

    /// Exports identity as encrypted backup.
    ///
    /// The backup contains the master seed encrypted with a key derived from the password.
    pub fn export_backup(&self, password: &str) -> Result<IdentityBackup, IdentityError> {
        // Validate password strength
        if password.len() < MIN_PASSWORD_LENGTH {
            return Err(IdentityError::WeakPassword);
        }

        // Generate random salt
        let rng = SystemRandom::new();
        let salt = ring::rand::generate::<[u8; 16]>(&rng)
            .map_err(|_| IdentityError::BackupFailed)?
            .expose();

        // Derive encryption key from password using PBKDF2
        let mut key_bytes = [0u8; 32];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
            &salt,
            password.as_bytes(),
            &mut key_bytes,
        );
        let encryption_key = SymmetricKey::from_bytes(key_bytes);

        // Prepare backup data: display_name_len (4 bytes) || display_name || master_seed
        let name_bytes = self.display_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();
        let mut plaintext = Vec::with_capacity(4 + name_bytes.len() + 32);
        plaintext.extend_from_slice(&name_len);
        plaintext.extend_from_slice(name_bytes);
        plaintext.extend_from_slice(&self.master_seed);

        // Encrypt the data
        let ciphertext = encrypt(&encryption_key, &plaintext)
            .map_err(|_| IdentityError::BackupFailed)?;

        // Backup format: salt (16 bytes) || ciphertext
        let mut backup_data = Vec::with_capacity(16 + ciphertext.len());
        backup_data.extend_from_slice(&salt);
        backup_data.extend_from_slice(&ciphertext);

        Ok(IdentityBackup::new(backup_data))
    }

    /// Imports identity from encrypted backup.
    pub fn import_backup(backup: &IdentityBackup, password: &str) -> Result<Self, IdentityError> {
        let data = backup.as_bytes();

        // Minimum size: salt (16) + nonce (12) + tag (16) + min data
        if data.len() < 16 + 12 + 16 + 4 + 32 {
            return Err(IdentityError::RestoreFailed);
        }

        // Extract salt
        let salt: [u8; 16] = data[..16]
            .try_into()
            .map_err(|_| IdentityError::RestoreFailed)?;

        // Derive decryption key from password
        let mut key_bytes = [0u8; 32];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
            &salt,
            password.as_bytes(),
            &mut key_bytes,
        );
        let decryption_key = SymmetricKey::from_bytes(key_bytes);

        // Decrypt the data
        let plaintext = decrypt(&decryption_key, &data[16..])
            .map_err(|_| IdentityError::RestoreFailed)?;

        // Parse the plaintext
        if plaintext.len() < 4 + 32 {
            return Err(IdentityError::RestoreFailed);
        }

        let name_len = u32::from_le_bytes(
            plaintext[..4].try_into().map_err(|_| IdentityError::RestoreFailed)?
        ) as usize;

        if plaintext.len() < 4 + name_len + 32 {
            return Err(IdentityError::RestoreFailed);
        }

        let display_name = String::from_utf8(plaintext[4..4 + name_len].to_vec())
            .map_err(|_| IdentityError::RestoreFailed)?;

        let master_seed: [u8; 32] = plaintext[4 + name_len..4 + name_len + 32]
            .try_into()
            .map_err(|_| IdentityError::RestoreFailed)?;

        Ok(Self::from_seed(master_seed, display_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_identity() {
        let identity = Identity::create("Test User");
        assert_eq!(identity.display_name(), "Test User");
    }

    #[test]
    fn test_backup_restore_roundtrip() {
        let original = Identity::create("Alice");
        let password = "SecurePassword123";
        let backup = original.export_backup(password).unwrap();
        let restored = Identity::import_backup(&backup, password).unwrap();
        assert_eq!(original.public_id(), restored.public_id());
    }
}
