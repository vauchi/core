// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device link response (from existing device containing encrypted seed).

use zeroize::Zeroize;

use crate::crypto::{SymmetricKey, decrypt, encrypt};
use crate::identity::DeviceRegistry;

use super::super::ExchangeError;

/// Response from existing device containing the encrypted seed.
///
/// ## Security Note (Tracker #31)
///
/// The raw 32-byte master seed is transmitted, encrypted only by the ephemeral
/// QR link key. A compromised new device gains full key derivation capability
/// for ALL device indices (past and future). The intended mitigation is to
/// replace this with a device-specific derived subkey (HKDF from master_seed
/// with device_index binding), limiting a compromised device to its own keys.
/// Security: Clone intentionally omitted — cloning would create a second copy of
/// `master_seed` that is independently zeroized, defeating the zeroization guarantee.
pub struct DeviceLinkResponse {
    /// The master seed (encrypted with link key before transmission)
    master_seed: [u8; 32],
    /// Identity display name
    display_name: String,
    /// Assigned device index for the new device
    device_index: u32,
    /// Current device registry
    registry: DeviceRegistry,
    /// Sync payload containing contacts, own card, owner-private tags/places,
    /// exchange locations, and ratchet states (optional, may be empty).
    sync_payload_json: String,
}

impl DeviceLinkResponse {
    /// Creates a new device link response.
    ///
    /// The existing device creates this with its master seed and the next
    /// available device index.
    pub fn new(
        master_seed: [u8; 32],
        display_name: String,
        device_index: u32,
        registry: DeviceRegistry,
    ) -> Self {
        DeviceLinkResponse {
            master_seed,
            display_name,
            device_index,
            registry,
            sync_payload_json: String::new(),
        }
    }

    /// Creates a new device link response with sync payload.
    pub fn with_sync_payload(
        master_seed: [u8; 32],
        display_name: String,
        device_index: u32,
        registry: DeviceRegistry,
        sync_payload_json: String,
    ) -> Self {
        DeviceLinkResponse {
            master_seed,
            display_name,
            device_index,
            registry,
            sync_payload_json,
        }
    }

    /// Returns the master seed.
    pub fn master_seed(&self) -> &[u8; 32] {
        &self.master_seed
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the device index.
    pub fn device_index(&self) -> u32 {
        self.device_index
    }

    /// Returns the device registry.
    pub fn registry(&self) -> &DeviceRegistry {
        &self.registry
    }

    /// Returns the sync payload JSON (may be empty).
    pub fn sync_payload_json(&self) -> &str {
        &self.sync_payload_json
    }

    /// Serializes the response for transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        let name_bytes = self.display_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();
        let registry_json = self.registry.to_json();
        let registry_bytes = registry_json.as_bytes();
        let registry_len = (registry_bytes.len() as u32).to_le_bytes();
        let sync_bytes = self.sync_payload_json.as_bytes();
        let sync_len = (sync_bytes.len() as u32).to_le_bytes();

        let mut data = Vec::new();
        data.extend_from_slice(&self.master_seed);
        data.extend_from_slice(&name_len);
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(&self.device_index.to_le_bytes());
        data.extend_from_slice(&registry_len);
        data.extend_from_slice(registry_bytes);
        data.extend_from_slice(&sync_len);
        data.extend_from_slice(sync_bytes);
        data
    }

    /// Deserializes a response from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ExchangeError> {
        if data.len() < 32 + 4 + 4 + 4 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let master_seed: [u8; 32] = data[..32]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let name_len = u32::from_le_bytes(
            data[32..36]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        ) as usize;

        if data.len() < 32 + 4 + name_len + 4 + 4 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let display_name = String::from_utf8(data[36..36 + name_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let offset = 36 + name_len;
        let device_index = u32::from_le_bytes(
            data[offset..offset + 4]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        );

        let registry_len = u32::from_le_bytes(
            data[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        ) as usize;

        if data.len() < offset + 8 + registry_len {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let registry_json = String::from_utf8(data[offset + 8..offset + 8 + registry_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let registry = DeviceRegistry::from_json(&registry_json)
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        // Parse sync payload (optional, may be empty or missing in older formats)
        let sync_offset = offset + 8 + registry_len;
        let sync_payload_json = if data.len() >= sync_offset + 4 {
            let sync_len = u32::from_le_bytes(
                data[sync_offset..sync_offset + 4]
                    .try_into()
                    .map_err(|_| ExchangeError::InvalidQRFormat)?,
            ) as usize;

            if data.len() >= sync_offset + 4 + sync_len {
                String::from_utf8(data[sync_offset + 4..sync_offset + 4 + sync_len].to_vec())
                    .map_err(|_| ExchangeError::InvalidQRFormat)?
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        Ok(DeviceLinkResponse {
            master_seed,
            display_name,
            device_index,
            registry,
            sync_payload_json,
        })
    }

    /// Encrypts the response using the link key.
    pub fn encrypt(&self, link_key: &[u8; 32]) -> Result<Vec<u8>, ExchangeError> {
        let key = SymmetricKey::from_bytes(*link_key);
        let plaintext = self.to_bytes();
        encrypt(&key, &plaintext).map_err(|_| ExchangeError::CryptoError)
    }

    /// Decrypts a response using the link key.
    pub fn decrypt(ciphertext: &[u8], link_key: &[u8; 32]) -> Result<Self, ExchangeError> {
        let key = SymmetricKey::from_bytes(*link_key);
        let plaintext = decrypt(&key, ciphertext).map_err(|_| ExchangeError::CryptoError)?;
        Self::from_bytes(&plaintext)
    }
}

impl Drop for DeviceLinkResponse {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}
