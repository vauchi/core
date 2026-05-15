// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity Management Module
//!
//! Handles user identity creation, backup, and restoration.
//! Each identity has a unique Ed25519 signing keypair and X25519 exchange keypair.
//!
//! Backup format:
//! - v2 (current): Argon2id KDF + XChaCha20-Poly1305 encryption

#[cfg(feature = "testing")]
pub mod backup;
#[cfg(not(feature = "testing"))]
mod backup;
pub mod device;
pub mod password;

pub use backup::IdentityBackup;
pub use device::{
    BroadcastDevice, DeviceError, DeviceInfo, DeviceRegistry, DeviceRevocationCertificate,
    DeviceType, MAX_DEVICES, RegisteredDevice, RegistryBroadcast, check_identity_collision,
    classify_device_type,
};

use crate::crypto::X3DHKeyPair;
use crate::crypto::{
    HKDF, Signature, SigningKeyPair, decrypt, derive_key_argon2id, encrypt, random_bytes,
};
use crate::text::normalize_text;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

/// Identity-related errors.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum IdentityError {
    #[error("Display name cannot be empty")]
    EmptyDisplayName,
    #[error("Password too weak: requires minimum 8 characters and zxcvbn score >= 3")]
    WeakPassword,
    #[error("Backup encryption failed")]
    BackupFailed,
    #[error("Invalid backup or wrong password")]
    RestoreFailed,
}

/// Backup format version byte for Argon2id + XChaCha20.
const BACKUP_VERSION_V2: u8 = 0x02;

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
    pub fn create(display_name: &str, now: u64) -> Self {
        // Generate random master seed
        let master_seed: [u8; 32] = random_bytes();

        Self::from_seed(master_seed, normalize_text(display_name), now)
    }

    /// Creates an identity from an existing seed with default device index 0.
    fn from_seed(master_seed: [u8; 32], display_name: String, now: u64) -> Self {
        Self::from_seed_with_device(
            master_seed,
            display_name,
            0,
            "Primary Device".to_string(),
            now,
        )
    }

    /// Creates an identity from a device link response.
    ///
    /// Used when joining an existing identity from another device.
    /// The master seed and device index come from the device link response,
    /// while the device name is chosen by the user for this device.
    pub fn from_device_link(
        master_seed: [u8; 32],
        display_name: String,
        device_index: u32,
        device_name: String,
        now: u64,
    ) -> Self {
        Self::from_seed_with_device(
            master_seed,
            normalize_text(&display_name),
            device_index,
            device_name,
            now,
        )
    }

    /// Creates an identity from an existing seed with specific device info.
    fn from_seed_with_device(
        master_seed: [u8; 32],
        display_name: String,
        device_index: u32,
        device_name: String,
        now: u64,
    ) -> Self {
        // Derive signing keypair from master seed
        let signing_keypair = SigningKeyPair::from_seed(&master_seed);

        // Cache the signing public key bytes
        let signing_public_key = *signing_keypair.public_key().as_bytes();

        // Derive exchange keypair using HKDF with domain separation
        // master_seed is IKM (high-entropy input), no salt needed
        let exchange_seed = HKDF::derive_key(None, &master_seed, b"Vauchi_Exchange_Seed_v2");

        // Create X25519 keypair and store the actual public key
        let x3dh = X3DHKeyPair::from_bytes(*exchange_seed);
        let exchange_public_key = *x3dh.public_key();

        // Create device info for this device
        let device_info = DeviceInfo::derive(&master_seed, device_index, device_name, now);

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
        self.display_name = normalize_text(name);
    }

    /// Tries to set the display name, returning an error if invalid.
    pub fn try_set_display_name(&mut self, name: &str) -> Result<(), IdentityError> {
        let normalized = normalize_text(name);
        if normalized.is_empty() {
            return Err(IdentityError::EmptyDisplayName);
        }
        self.display_name = normalized;
        Ok(())
    }

    /// Returns a reference to the master seed.
    ///
    /// Used for mailbox token derivation (SP-33). The caller must not
    /// persist or log the seed — it is zeroized on drop.
    pub fn master_seed(&self) -> &[u8; 32] {
        &self.master_seed
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
        let x25519_secret = HKDF::derive_key(None, &self.master_seed, b"Vauchi_Exchange_Seed_v2");
        X3DHKeyPair::from_bytes(*x25519_secret)
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

    /// Creates a fresh DeviceInfo for this device.
    ///
    /// This is useful when you need to pass DeviceInfo by value (e.g., to
    /// DeviceSyncOrchestrator) since DeviceInfo doesn't implement Clone
    /// for security reasons.
    pub fn create_device_info(&self, now: u64) -> DeviceInfo {
        DeviceInfo::derive(
            &self.master_seed,
            self.device_info.device_index(),
            self.device_info.device_name().to_string(),
            now,
        )
    }

    /// Derives the Shredding Master Key (SMK) from this identity's master seed.
    ///
    /// Called once during identity creation or migration to SMK-based encryption.
    /// The returned SMK should be immediately stored in SecureStorage.
    pub fn derive_smk(&self) -> crate::crypto::ShreddingMasterKey {
        crate::crypto::ShreddingMasterKey::derive_from_seed(&self.master_seed)
    }

    /// Creates the initial device registry containing only this device.
    pub fn initial_device_registry(&self) -> DeviceRegistry {
        DeviceRegistry::new(
            self.device_info.to_registered(&self.master_seed),
            &self.signing_keypair,
        )
    }

    /// Creates a device link initiator for linking a new device.
    ///
    /// This generates a QR code that can be scanned by a new device
    /// to receive the identity's master seed.
    pub fn create_device_link_initiator(
        &self,
        registry: DeviceRegistry,
        now: u64,
    ) -> crate::exchange::DeviceLinkInitiator {
        crate::exchange::DeviceLinkInitiator::new(self.master_seed, self, registry, now)
    }

    /// Restores a device link initiator from a saved QR code.
    ///
    /// Used when the QR was generated earlier and saved, then the
    /// request comes in later.
    pub fn restore_device_link_initiator(
        &self,
        registry: DeviceRegistry,
        qr: crate::exchange::DeviceLinkQR,
    ) -> crate::exchange::DeviceLinkInitiatorRestored {
        crate::exchange::DeviceLinkInitiatorRestored::new(self.master_seed, self, registry, qr)
    }

    /// Serializes the identity to bytes for storage persistence.
    ///
    /// Unlike `export_backup`, this does NOT password-encrypt the data.
    /// The caller (Storage) is responsible for encrypting with the storage key.
    ///
    /// Format: same as backup plaintext — `name_len (4) || name || master_seed (32)
    /// || device_index (4) || device_name_len (4) || device_name`
    pub fn to_storage_bytes(&self) -> Vec<u8> {
        let name_bytes = self.display_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();
        let device_name_bytes = self.device_info.device_name().as_bytes();
        let device_name_len = (device_name_bytes.len() as u32).to_le_bytes();
        let device_index = self.device_info.device_index().to_le_bytes();

        let mut buf =
            Vec::with_capacity(4 + name_bytes.len() + 32 + 4 + 4 + device_name_bytes.len());
        buf.extend_from_slice(&name_len);
        buf.extend_from_slice(name_bytes);
        buf.extend_from_slice(&self.master_seed);
        buf.extend_from_slice(&device_index);
        buf.extend_from_slice(&device_name_len);
        buf.extend_from_slice(device_name_bytes);
        buf
    }

    /// Restores an identity from storage bytes (inverse of `to_storage_bytes`).
    pub fn from_storage_bytes(data: &[u8], now: u64) -> Result<Self, IdentityError> {
        Self::parse_backup_plaintext(data, now)
    }

    /// Exports identity as encrypted backup (v2: Argon2id + XChaCha20-Poly1305).
    ///
    /// The backup contains the master seed encrypted with a key derived from the password.
    /// Requires a strong password (zxcvbn score >= 3).
    ///
    /// Backup format: `version_byte (0x02) || salt (16 bytes) || ciphertext`
    pub fn export_backup(&self, password: &str) -> Result<IdentityBackup, IdentityError> {
        // Validate password strength using zxcvbn
        password::validate_password(password)?;

        // Generate random salt
        let salt: [u8; 16] = random_bytes();

        // Derive encryption key from password using Argon2id
        let encryption_key = derive_key_argon2id(password.as_bytes(), &salt)
            .map_err(|_| IdentityError::BackupFailed)?;

        // Prepare backup data:
        // display_name_len (4 bytes) || display_name || master_seed (32 bytes)
        // || device_index (4 bytes) || device_name_len (4 bytes) || device_name
        let name_bytes = self.display_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();
        let device_name_bytes = self.device_info.device_name().as_bytes();
        let device_name_len = (device_name_bytes.len() as u32).to_le_bytes();
        let device_index = self.device_info.device_index().to_le_bytes();

        // Wrap in Zeroizing to ensure master_seed bytes are zeroized after encryption
        let mut plaintext = Zeroizing::new(Vec::with_capacity(
            4 + name_bytes.len() + 32 + 4 + 4 + device_name_bytes.len(),
        ));
        plaintext.extend_from_slice(&name_len);
        plaintext.extend_from_slice(name_bytes);
        plaintext.extend_from_slice(&self.master_seed);
        plaintext.extend_from_slice(&device_index);
        plaintext.extend_from_slice(&device_name_len);
        plaintext.extend_from_slice(device_name_bytes);

        // Encrypt the data (uses XChaCha20-Poly1305)
        let ciphertext =
            encrypt(&encryption_key, &plaintext).map_err(|_| IdentityError::BackupFailed)?;

        // Backup format: version_byte || salt (16 bytes) || ciphertext
        let mut backup_data = Vec::with_capacity(1 + 16 + ciphertext.len());
        backup_data.push(BACKUP_VERSION_V2);
        backup_data.extend_from_slice(&salt);
        backup_data.extend_from_slice(&ciphertext);

        Ok(IdentityBackup::new(backup_data))
    }

    /// Imports identity from encrypted v2 backup (Argon2id + XChaCha20-Poly1305).
    ///
    /// ## Security Note (Tracker #69)
    ///
    /// The restored identity has the same `master_seed` and therefore the same
    /// signing key pair as the original. Two devices importing the same backup
    /// are cryptographically indistinguishable — there is no protocol-level
    /// mechanism to detect or prevent "identity clones" operating independently.
    /// The restored device also starts with the backup's `device_index`, not a
    /// fresh one, so it can impersonate the original device.
    pub fn import_backup(
        backup: &IdentityBackup,
        password: &str,
        now: u64,
    ) -> Result<Self, IdentityError> {
        let data = backup.as_bytes();

        if data.is_empty() {
            return Err(IdentityError::RestoreFailed);
        }

        match data[0] {
            BACKUP_VERSION_V2 => Self::import_backup_v2(&data[1..], password, now),
            _ => Err(IdentityError::RestoreFailed),
        }
    }

    /// Imports v2 backup (Argon2id + XChaCha20-Poly1305).
    ///
    /// Data format: `salt (16 bytes) || ciphertext`
    fn import_backup_v2(data: &[u8], password: &str, now: u64) -> Result<Self, IdentityError> {
        // salt (16) + at least some ciphertext
        if data.len() < 16 + 1 + 24 + 16 + 4 + 32 {
            return Err(IdentityError::RestoreFailed);
        }

        let salt: [u8; 16] = data[..16]
            .try_into()
            .map_err(|_| IdentityError::RestoreFailed)?;

        // Derive decryption key using Argon2id
        let decryption_key = derive_key_argon2id(password.as_bytes(), &salt)
            .map_err(|_| IdentityError::RestoreFailed)?;

        // Decrypt (auto-detects tagged XChaCha20-Poly1305)
        // Wrap in Zeroizing to ensure plaintext (containing master_seed) is zeroized on drop
        let plaintext = Zeroizing::new(
            decrypt(&decryption_key, &data[16..]).map_err(|_| IdentityError::RestoreFailed)?,
        );

        Self::parse_backup_plaintext(&plaintext, now)
    }

    /// Parses the decrypted backup plaintext into an Identity.
    fn parse_backup_plaintext(plaintext: &[u8], now: u64) -> Result<Self, IdentityError> {
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
            now,
        ))
    }
}
