// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! NFC Card Payload
//!
//! Plaintext data envelope encrypted before NFC transmission.
//! Contains sender identity, display name, exchange key, and CRC16 sanity check.
//! Security integrity is provided by AEAD (XChaCha20-Poly1305), not CRC16.

use serde::{Deserialize, Serialize};

/// Plaintext card data exchanged during NFC tap.
///
/// Serialized with `postcard`, then encrypted with XChaCha20-Poly1305.
/// The CRC16 field is computed over the serialized identity_key + display_name +
/// exchange_key and verified after decryption as a sanity check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfcCardPayload {
    pub identity_key: [u8; 32],
    pub display_name: String,
    pub exchange_key: [u8; 32],
    pub crc16: u16,
}

impl NfcCardPayload {
    /// Creates a new payload, computing CRC16 automatically.
    pub fn new(identity_key: [u8; 32], display_name: String, exchange_key: [u8; 32]) -> Self {
        let crc = Self::compute_crc16(&identity_key, &display_name, &exchange_key);
        Self {
            identity_key,
            display_name,
            exchange_key,
            crc16: crc,
        }
    }

    /// Returns the CRC16 value.
    pub fn crc16(&self) -> u16 {
        self.crc16
    }

    /// Verifies the CRC16 matches the payload data.
    pub fn verify_crc16(&self) -> bool {
        let expected =
            Self::compute_crc16(&self.identity_key, &self.display_name, &self.exchange_key);
        expected == self.crc16
    }

    /// Serializes to bytes using postcard.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    /// Deserializes from bytes using postcard.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }

    /// CRC16/CCITT-FALSE over concatenated fields.
    fn compute_crc16(identity_key: &[u8; 32], display_name: &str, exchange_key: &[u8; 32]) -> u16 {
        let mut crc: u16 = 0xFFFF;
        for &byte in identity_key
            .iter()
            .chain(display_name.as_bytes())
            .chain(exchange_key.iter())
        {
            crc ^= (byte as u16) << 8;
            for _ in 0..8 {
                if crc & 0x8000 != 0 {
                    crc = (crc << 1) ^ 0x1021;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }
}
