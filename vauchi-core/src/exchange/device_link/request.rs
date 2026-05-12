// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device link request (from new device).

use crate::crypto::{SymmetricKey, decrypt, encrypt};

use super::super::ExchangeError;

/// Request from new device to link with existing identity.
#[derive(Clone, Debug)]
pub struct DeviceLinkRequest {
    /// New device's proposed name
    pub device_name: String,
    /// Random nonce to prevent replay attacks
    pub nonce: [u8; 32],
    /// Timestamp of request
    pub timestamp: u64,
}

impl DeviceLinkRequest {
    /// Creates a new device link request.
    pub fn new(device_name: String) -> Self {
        let nonce: [u8; 32] = crate::crypto::random_bytes();

        let timestamp = crate::exchange::now_secs();

        DeviceLinkRequest {
            device_name,
            nonce,
            timestamp,
        }
    }

    /// Serializes the request for transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        let name_bytes = self.device_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();

        let mut data = Vec::new();
        data.extend_from_slice(&name_len);
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(&self.nonce);
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data
    }

    /// Deserializes a request from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ExchangeError> {
        if data.len() < 4 + 32 + 8 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let name_len = u32::from_le_bytes(
            data[..4]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        ) as usize;

        if data.len() < 4 + name_len + 32 + 8 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let device_name = String::from_utf8(data[4..4 + name_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let nonce: [u8; 32] = data[4 + name_len..4 + name_len + 32]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let timestamp = u64::from_le_bytes(
            data[4 + name_len + 32..4 + name_len + 40]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        );

        Ok(DeviceLinkRequest {
            device_name,
            nonce,
            timestamp,
        })
    }

    /// Encrypts the request using the link key from the QR.
    pub fn encrypt(&self, link_key: &[u8; 32]) -> Result<Vec<u8>, ExchangeError> {
        let key = SymmetricKey::from_bytes(*link_key);
        let plaintext = self.to_bytes();
        encrypt(&key, &plaintext).map_err(|_| ExchangeError::CryptoError)
    }

    /// Decrypts a request using the link key.
    pub fn decrypt(ciphertext: &[u8], link_key: &[u8; 32]) -> Result<Self, ExchangeError> {
        let key = SymmetricKey::from_bytes(*link_key);
        let plaintext = decrypt(&key, ciphertext).map_err(|_| ExchangeError::CryptoError)?;
        Self::from_bytes(&plaintext)
    }
}
