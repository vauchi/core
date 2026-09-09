// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! NFC Active Exchange Module
//!
//! Phone-to-phone NFC tap exchange. A single tap replaces both QR scan
//! and proximity verification. Both devices use fresh ephemeral X25519
//! keys for full forward secrecy.
//!
//! Magic bytes: "VNFC"
//! Payload size: 174 bytes

use super::ExchangeError;
use super::exchange_payload::{
    EXCHANGE_PAYLOAD_SIZE, ParsedPayload, build_exchange_payload, is_payload_expired,
    parse_exchange_payload, verify_payload_signature,
};
use crate::identity::Identity;

use super::x3dh::X3DHKeyPair;

/// NFC payload magic bytes.
const NFC_MAGIC: &[u8; 4] = b"VNFC";

/// NFC payload expiry in seconds (60 seconds — tighter than QR).
const NFC_EXPIRY_SECONDS: u64 = 60;

/// NFC payload size in bytes.
pub const NFC_PAYLOAD_SIZE: usize = EXCHANGE_PAYLOAD_SIZE;

/// NFC exchange payload.
///
/// 174-byte payload exchanged during an NFC tap:
/// - Magic "VNFC" (4 bytes)
/// - Version (1 byte)
/// - Flags (1 byte)
/// - Identity key — Ed25519 signing public key (32 bytes)
/// - Exchange key — fresh ephemeral X25519 public key (32 bytes)
/// - Token — random session token (32 bytes)
/// - Timestamp — Unix timestamp (8 bytes)
/// - Signature — Ed25519 signature over all preceding fields (64 bytes)
#[derive(Clone, Debug)]
pub struct ExchangeNfc {
    inner: ParsedPayload,
}

impl ExchangeNfc {
    /// Generates a new NFC exchange payload.
    pub fn generate(identity: &Identity, ephemeral: &X3DHKeyPair, now: u64) -> Self {
        use crate::crypto::random_bytes;

        let token: [u8; 32] = random_bytes();

        let timestamp = now;

        Self::generate_with_timestamp(identity, ephemeral, token, timestamp)
    }

    /// Generates with explicit timestamp (for testing).
    pub fn generate_with_timestamp(
        identity: &Identity,
        ephemeral: &X3DHKeyPair,
        token: [u8; 32],
        timestamp: u64,
    ) -> Self {
        let bytes = build_exchange_payload(NFC_MAGIC, identity, ephemeral, token, timestamp);
        let inner = parse_exchange_payload(&bytes, NFC_MAGIC, ExchangeError::InvalidNfcFormat)
            .expect("Freshly built payload should parse");
        ExchangeNfc { inner }
    }

    /// Returns the identity (Ed25519 signing) key.
    pub fn identity_key(&self) -> &[u8; 32] {
        &self.inner.identity_key
    }

    /// Returns the exchange (X25519 ephemeral) key.
    pub fn exchange_key(&self) -> &[u8; 32] {
        &self.inner.exchange_key
    }

    /// Returns the session token.
    pub fn token(&self) -> &[u8; 32] {
        &self.inner.token
    }

    /// Returns the timestamp.
    pub fn timestamp(&self) -> u64 {
        self.inner.timestamp
    }

    /// Checks if the payload has expired.
    pub fn is_expired(&self, now: u64) -> bool {
        is_payload_expired(self.inner.timestamp, NFC_EXPIRY_SECONDS, now)
    }

    /// Verifies the Ed25519 signature.
    pub fn verify_signature(&self) -> bool {
        verify_payload_signature(NFC_MAGIC, &self.inner)
    }

    /// Serializes the payload to bytes.
    pub fn to_bytes(&self) -> [u8; NFC_PAYLOAD_SIZE] {
        let mut buf = [0u8; NFC_PAYLOAD_SIZE];
        buf[0..4].copy_from_slice(NFC_MAGIC);
        buf[4] = self.inner.version;
        buf[5] = self.inner.flags;
        buf[6..38].copy_from_slice(&self.inner.identity_key);
        buf[38..70].copy_from_slice(&self.inner.exchange_key);
        buf[70..102].copy_from_slice(&self.inner.token);
        buf[102..110].copy_from_slice(&self.inner.timestamp.to_be_bytes());
        buf[110..174].copy_from_slice(&self.inner.signature);
        buf
    }

    /// Parses the payload from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ExchangeError> {
        let inner = parse_exchange_payload(bytes, NFC_MAGIC, ExchangeError::InvalidNfcFormat)?;
        Ok(ExchangeNfc { inner })
    }
}
