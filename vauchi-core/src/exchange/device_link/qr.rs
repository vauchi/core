// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device link QR code generation and parsing.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

use super::types::{DEVICE_LINK_MAGIC, DEVICE_LINK_VERSION, LINK_QR_EXPIRY_SECONDS};
use crate::crypto::{PublicKey, Signature};
use crate::identity::Identity;

use super::super::ExchangeError;

/// Device link QR code data structure.
///
/// Displayed on the existing device for a new device to scan.
/// Contains a random link key used to encrypt the seed transfer.
///
/// Security: Clone intentionally omitted — cloning would create a second copy of
/// `link_key` that is independently zeroized, defeating the zeroization guarantee.
#[derive(Debug)]
pub struct DeviceLinkQR {
    /// Protocol version
    pub(super) version: u8,
    /// Identity's Ed25519 public key (so new device knows which identity)
    pub(super) identity_public_key: [u8; 32],
    /// Random link key for encrypting the seed transfer (32 bytes)
    pub(super) link_key: [u8; 32],
    /// Unix timestamp when QR was generated
    pub(super) timestamp: u64,
    /// Signature over the above fields (proves identity ownership)
    pub(super) signature: [u8; 64],
}

impl DeviceLinkQR {
    /// Generates a new device link QR code for the given identity.
    pub fn generate(identity: &Identity) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self::generate_with_timestamp(identity, timestamp)
    }

    /// Generates a link QR with a specific timestamp (for testing).
    pub fn generate_with_timestamp(identity: &Identity, timestamp: u64) -> Self {
        // Generate random link key
        let link_key: [u8; 32] = crate::crypto::random_bytes();

        let identity_public_key = *identity.signing_public_key();

        // Create message to sign
        let mut message = Vec::new();
        message.push(DEVICE_LINK_VERSION);
        message.extend_from_slice(&identity_public_key);
        message.extend_from_slice(&link_key);
        message.extend_from_slice(&timestamp.to_be_bytes());

        // Sign the message
        let signature = identity.sign(&message);

        DeviceLinkQR {
            version: DEVICE_LINK_VERSION,
            identity_public_key,
            link_key,
            timestamp,
            signature: *signature.as_bytes(),
        }
    }

    /// Returns the identity public key.
    pub fn identity_public_key(&self) -> &[u8; 32] {
        &self.identity_public_key
    }

    /// Returns the link key (used for encrypting seed transfer).
    pub fn link_key(&self) -> &[u8; 32] {
        &self.link_key
    }

    /// Returns the timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Checks if the QR code has expired.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        now > self.timestamp + LINK_QR_EXPIRY_SECONDS
    }

    /// Verifies the signature on the QR code.
    pub fn verify_signature(&self) -> bool {
        let mut message = Vec::new();
        message.push(self.version);
        message.extend_from_slice(&self.identity_public_key);
        message.extend_from_slice(&self.link_key);
        message.extend_from_slice(&self.timestamp.to_be_bytes());

        let public_key = PublicKey::from_bytes(self.identity_public_key);
        let signature = Signature::from_bytes(self.signature);

        public_key.verify(&message, &signature)
    }

    /// Encodes the QR data to a string for embedding in QR code.
    pub fn to_data_string(&self) -> String {
        // Format: base64(MAGIC || version || identity_key || link_key || timestamp || signature)
        let mut data = Vec::new();
        data.extend_from_slice(DEVICE_LINK_MAGIC);
        data.push(self.version);
        data.extend_from_slice(&self.identity_public_key);
        data.extend_from_slice(&self.link_key);
        data.extend_from_slice(&self.timestamp.to_be_bytes());
        data.extend_from_slice(&self.signature);

        BASE64.encode(&data)
    }

    /// Parses QR data from a scanned string.
    pub fn from_data_string(data: &str) -> Result<Self, ExchangeError> {
        let bytes = BASE64
            .decode(data)
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        // Minimum length: MAGIC(4) + version(1) + identity_key(32) + link_key(32) + timestamp(8) + sig(64) = 141
        if bytes.len() < 141 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        // Check magic bytes
        if &bytes[0..4] != DEVICE_LINK_MAGIC {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let version = bytes[4];
        if version != DEVICE_LINK_VERSION {
            return Err(ExchangeError::InvalidProtocolVersion);
        }

        let identity_public_key: [u8; 32] = bytes[5..37]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let link_key: [u8; 32] = bytes[37..69]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let timestamp = u64::from_be_bytes(
            bytes[69..77]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        );

        let signature: [u8; 64] = bytes[77..141]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let qr = DeviceLinkQR {
            version,
            identity_public_key,
            link_key,
            timestamp,
            signature,
        };

        // Verify signature
        if !qr.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        Ok(qr)
    }

    /// Generates an actual QR code image as a string representation.
    pub fn to_qr_image_string(&self) -> String {
        use qrcode::QrCode;

        let data = self.to_data_string();
        let code = QrCode::new(&data).expect("QR generation should not fail");

        code.render()
            .light_color(' ')
            .dark_color('█')
            .quiet_zone(false)
            .build()
    }

    /// Returns a human-readable fingerprint of the identity public key.
    ///
    /// Format: first 8 bytes of the identity public key, hex-encoded with
    /// separators every 4 hex chars: `AB12-CD34-EF56-7890`.
    pub fn identity_fingerprint(&self) -> String {
        let bytes = &self.identity_public_key[..8];
        let hex: Vec<String> = bytes
            .chunks(2)
            .map(|c| format!("{:02X}{:02X}", c[0], c[1]))
            .collect();
        hex.join("-")
    }
}

impl Drop for DeviceLinkQR {
    fn drop(&mut self) {
        self.link_key.zeroize();
    }
}
