//! Identity Management Module
//!
//! Handles user identity creation, backup, and restoration.
//! Each identity has a unique Ed25519 signing keypair and X25519 exchange keypair.

mod backup;
pub mod device;

pub use backup::IdentityBackup;
pub use device::{
    BroadcastDevice, DeviceError, DeviceInfo, DeviceRegistry, DeviceRevocationCertificate,
    RegisteredDevice, RegistryBroadcast, MAX_DEVICES,
};

use crate::crypto::{decrypt, encrypt, Signature, SigningKeyPair, SymmetricKey, HKDF};
use crate::exchange::X3DHKeyPair;
use ring::pbkdf2;
use ring::rand::SystemRandom;
use std::num::NonZeroU32;
use thiserror::Error;
use zeroize::Zeroize;

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
    /// Device-specific information for this device.
    device_info: DeviceInfo,
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

    /// Creates an identity from an existing seed with default device index 0.
    fn from_seed(master_seed: [u8; 32], display_name: String) -> Self {
        Self::from_seed_with_device(master_seed, display_name, 0, "Primary Device".to_string())
    }

    /// Creates an identity from an existing seed with specific device info.
    fn from_seed_with_device(
        master_seed: [u8; 32],
        display_name: String,
        device_index: u32,
        device_name: String,
    ) -> Self {
        // Derive signing keypair from master seed
        let signing_keypair = SigningKeyPair::from_seed(&master_seed);

        // Cache the signing public key bytes
        let signing_public_key = *signing_keypair.public_key().as_bytes();

        // Derive exchange keypair using HKDF with domain separation
        let exchange_seed = HKDF::derive_key(
            Some(&master_seed),
            &[],
            b"WebBook_Exchange_Seed",
        );

        // Create X25519 keypair and store the actual public key
        let x3dh = X3DHKeyPair::from_bytes(exchange_seed);
        let exchange_public_key = *x3dh.public_key();

        // Create device info for this device
        let device_info = DeviceInfo::derive(&master_seed, device_index, device_name);

        Identity {
            master_seed,
            signing_keypair,
            signing_public_key,
            exchange_public_key,
            display_name,
            device_info,
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
    /// The keypair is derived from the master seed using HKDF with domain
    /// separation, ensuring consistency with exchange_public_key.
    pub fn x3dh_keypair(&self) -> X3DHKeyPair {
        // Derive X25519 secret from master_seed using HKDF
        // Uses same derivation as exchange_public_key for consistency
        let x25519_secret = HKDF::derive_key(
            Some(&self.master_seed),
            &[],
            b"WebBook_Exchange_Seed",
        );
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

    /// Returns the signing keypair reference.
    pub fn signing_keypair(&self) -> &SigningKeyPair {
        &self.signing_keypair
    }

    /// Returns the device info for this device.
    pub fn device_info(&self) -> &DeviceInfo {
        &self.device_info
    }

    /// Returns the device index for this device.
    pub fn device_index(&self) -> u32 {
        self.device_info.device_index()
    }

    /// Returns the device ID for this device.
    pub fn device_id(&self) -> &[u8; 32] {
        self.device_info.device_id()
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

        // Prepare backup data:
        // display_name_len (4 bytes) || display_name || master_seed (32 bytes)
        // || device_index (4 bytes) || device_name_len (4 bytes) || device_name
        let name_bytes = self.display_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();
        let device_name_bytes = self.device_info.device_name().as_bytes();
        let device_name_len = (device_name_bytes.len() as u32).to_le_bytes();
        let device_index = self.device_info.device_index().to_le_bytes();

        let mut plaintext =
            Vec::with_capacity(4 + name_bytes.len() + 32 + 4 + 4 + device_name_bytes.len());
        plaintext.extend_from_slice(&name_len);
        plaintext.extend_from_slice(name_bytes);
        plaintext.extend_from_slice(&self.master_seed);
        plaintext.extend_from_slice(&device_index);
        plaintext.extend_from_slice(&device_name_len);
        plaintext.extend_from_slice(device_name_bytes);

        // Encrypt the data
        let ciphertext =
            encrypt(&encryption_key, &plaintext).map_err(|_| IdentityError::BackupFailed)?;

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
        let plaintext =
            decrypt(&decryption_key, &data[16..]).map_err(|_| IdentityError::RestoreFailed)?;

        // Parse the plaintext
        if plaintext.len() < 4 + 32 {
            return Err(IdentityError::RestoreFailed);
        }

        let name_len = u32::from_le_bytes(
            plaintext[..4]
                .try_into()
                .map_err(|_| IdentityError::RestoreFailed)?,
        ) as usize;

        if plaintext.len() < 4 + name_len + 32 {
            return Err(IdentityError::RestoreFailed);
        }

        let display_name = String::from_utf8(plaintext[4..4 + name_len].to_vec())
            .map_err(|_| IdentityError::RestoreFailed)?;

        let master_seed: [u8; 32] = plaintext[4 + name_len..4 + name_len + 32]
            .try_into()
            .map_err(|_| IdentityError::RestoreFailed)?;

        // Parse device info (if present, for backward compatibility)
        let base_offset = 4 + name_len + 32;
        let (device_index, device_name) = if plaintext.len() >= base_offset + 8 {
            // New format with device info
            let device_index = u32::from_le_bytes(
                plaintext[base_offset..base_offset + 4]
                    .try_into()
                    .map_err(|_| IdentityError::RestoreFailed)?,
            );

            let device_name_len = u32::from_le_bytes(
                plaintext[base_offset + 4..base_offset + 8]
                    .try_into()
                    .map_err(|_| IdentityError::RestoreFailed)?,
            ) as usize;

            if plaintext.len() < base_offset + 8 + device_name_len {
                return Err(IdentityError::RestoreFailed);
            }

            let device_name = String::from_utf8(
                plaintext[base_offset + 8..base_offset + 8 + device_name_len].to_vec(),
            )
            .map_err(|_| IdentityError::RestoreFailed)?;

            (device_index, device_name)
        } else {
            // Old format without device info - use defaults
            (0, "Primary Device".to_string())
        };

        Ok(Self::from_seed_with_device(
            master_seed,
            display_name,
            device_index,
            device_name,
        ))
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

    #[test]
    fn test_identity_has_device_info() {
        let identity = Identity::create("Alice");
        assert_eq!(identity.device_index(), 0);
        assert_eq!(identity.device_info().device_name(), "Primary Device");
    }

    #[test]
    fn test_backup_restore_preserves_device_info() {
        // Create identity with custom device info
        let master_seed = [0x42u8; 32];
        let original = Identity::from_seed_with_device(
            master_seed,
            "Alice".to_string(),
            3,
            "My Phone".to_string(),
        );

        let password = "SecurePassword123";
        let backup = original.export_backup(password).unwrap();
        let restored = Identity::import_backup(&backup, password).unwrap();

        assert_eq!(restored.device_index(), 3);
        assert_eq!(restored.device_info().device_name(), "My Phone");
        assert_eq!(restored.device_id(), original.device_id());
    }

    #[test]
    fn test_device_id_deterministic() {
        let identity1 = Identity::create("Alice");
        let identity2 = Identity::create("Bob");

        // Different identities have different device IDs
        assert_ne!(identity1.device_id(), identity2.device_id());
    }
}
