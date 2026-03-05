// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! QR Code Exchange Protocol
//!
//! Handles generation and parsing of exchange QR codes.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::time::{SystemTime, UNIX_EPOCH};

use super::x3dh::X3DHKeyPair;
use super::ExchangeError;
use crate::crypto::{PublicKey, Signature};
use crate::identity::Identity;

/// Protocol version for QR codes.
/// v1: Original format (signing key only)
/// v2: Added X25519 exchange key for X3DH
const PROTOCOL_VERSION: u8 = 2;

/// QR code expiration time in seconds.
///
/// Production default: 300s (5 minutes).
/// With `test-timings` feature: 5s (for fast e2e tests).
#[cfg(not(feature = "test-timings"))]
const QR_EXPIRY_SECONDS: u64 = 300;
#[cfg(feature = "test-timings")]
const QR_EXPIRY_SECONDS: u64 = 5;

/// QR code magic bytes to identify Vauchi QR codes.
const MAGIC: &[u8; 4] = b"WBEX";

/// Exchange QR code data structure.
///
/// Contains all information needed to initiate a contact exchange,
/// including the displayer's display name for immediate contact labeling.
#[derive(Clone, Debug)]
pub struct ExchangeQR {
    /// Protocol version
    version: u8,
    /// Initiator's Ed25519 public key (for identity/verification)
    public_key: [u8; 32],
    /// Initiator's X25519 exchange key (for X3DH key agreement)
    exchange_key: [u8; 32],
    /// Random token for this exchange session
    exchange_token: [u8; 32],
    /// Seed for audio proximity challenge
    audio_challenge: [u8; 16],
    /// Unix timestamp when QR was generated
    timestamp: u64,
    /// Displayer's display name (UTF-8, max 255 bytes)
    display_name: String,
    /// Signature over the above fields (including display_name)
    signature: [u8; 64],
}

impl ExchangeQR {
    /// Generates a new exchange QR code with a fresh ephemeral X25519 key.
    ///
    /// The exchange key in the QR is the provided ephemeral key, not the
    /// identity's static X3DH key. This gives full forward secrecy.
    pub fn generate(identity: &Identity, ephemeral: &X3DHKeyPair) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self::generate_with_timestamp(identity, ephemeral, timestamp)
    }

    /// Generates a QR code with a specific ephemeral keypair and timestamp (for testing).
    pub fn generate_with_timestamp(
        identity: &Identity,
        ephemeral: &X3DHKeyPair,
        timestamp: u64,
    ) -> Self {
        use ring::rand::SystemRandom;

        let rng = SystemRandom::new();

        let exchange_token = ring::rand::generate::<[u8; 32]>(&rng)
            .expect("RNG should not fail")
            .expose();

        let audio_challenge = ring::rand::generate::<[u8; 16]>(&rng)
            .expect("RNG should not fail")
            .expose();

        let public_key = *identity.signing_public_key();

        // Use the provided ephemeral key — NOT identity's static exchange key
        let exchange_key: [u8; 32] = *ephemeral.public_key();

        let display_name = identity.display_name().to_string();
        let name_bytes = display_name.as_bytes();
        let name_len = (name_bytes.len() as u16).min(255);

        let mut message = Vec::new();
        message.push(PROTOCOL_VERSION);
        message.extend_from_slice(&public_key);
        message.extend_from_slice(&exchange_key);
        message.extend_from_slice(&exchange_token);
        message.extend_from_slice(&audio_challenge);
        message.extend_from_slice(&timestamp.to_be_bytes());
        message.extend_from_slice(&name_len.to_be_bytes());
        message.extend_from_slice(&name_bytes[..name_len as usize]);

        let signature = identity.sign(&message);

        ExchangeQR {
            version: PROTOCOL_VERSION,
            public_key,
            exchange_key,
            exchange_token,
            audio_challenge,
            timestamp,
            display_name,
            signature: *signature.as_bytes(),
        }
    }

    /// Returns the Ed25519 signing public key (for identity verification).
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }

    /// Returns the X25519 exchange key (for X3DH key agreement).
    pub fn exchange_key(&self) -> &[u8; 32] {
        &self.exchange_key
    }

    /// Returns the exchange token.
    pub fn exchange_token(&self) -> &[u8; 32] {
        &self.exchange_token
    }

    /// Returns the audio challenge seed.
    pub fn audio_challenge(&self) -> &[u8; 16] {
        &self.audio_challenge
    }

    /// Returns the displayer's display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
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

        now > self.timestamp + QR_EXPIRY_SECONDS
    }

    /// Verifies the signature on the QR code.
    pub fn verify_signature(&self) -> bool {
        let name_bytes = self.display_name.as_bytes();
        let name_len = name_bytes.len() as u16;

        // Reconstruct the signed message (includes display_name)
        let mut message = Vec::new();
        message.push(self.version);
        message.extend_from_slice(&self.public_key);
        message.extend_from_slice(&self.exchange_key);
        message.extend_from_slice(&self.exchange_token);
        message.extend_from_slice(&self.audio_challenge);
        message.extend_from_slice(&self.timestamp.to_be_bytes());
        message.extend_from_slice(&name_len.to_be_bytes());
        message.extend_from_slice(name_bytes);

        // Create public key for verification
        let public_key = PublicKey::from_bytes(self.public_key);
        let signature = Signature::from_bytes(self.signature);

        public_key.verify(&message, &signature)
    }

    /// Encodes the QR data to a string for embedding in QR code.
    pub fn to_data_string(&self) -> String {
        // Format: base64(MAGIC || version || pubkey || exchange_key || token || challenge || timestamp || name_len || name || signature)
        let name_bytes = self.display_name.as_bytes();
        let name_len = name_bytes.len() as u16;

        let mut data = Vec::new();
        data.extend_from_slice(MAGIC);
        data.push(self.version);
        data.extend_from_slice(&self.public_key);
        data.extend_from_slice(&self.exchange_key);
        data.extend_from_slice(&self.exchange_token);
        data.extend_from_slice(&self.audio_challenge);
        data.extend_from_slice(&self.timestamp.to_be_bytes());
        data.extend_from_slice(&name_len.to_be_bytes());
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(&self.signature);

        BASE64.encode(&data)
    }

    /// Parses QR data from a scanned string.
    pub fn from_data_string(data: &str) -> Result<Self, ExchangeError> {
        let bytes = BASE64
            .decode(data)
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        // Minimum length: MAGIC(4) + version(1) + pubkey(32) + exchange_key(32) + token(32) + challenge(16) + timestamp(8) + name_len(2) + sig(64) = 191
        if bytes.len() < 191 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        // Check magic bytes
        if &bytes[0..4] != MAGIC {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let version = bytes[4];
        if version != PROTOCOL_VERSION {
            return Err(ExchangeError::InvalidProtocolVersion);
        }

        let public_key: [u8; 32] = bytes[5..37]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let exchange_key: [u8; 32] = bytes[37..69]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let exchange_token: [u8; 32] = bytes[69..101]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let audio_challenge: [u8; 16] = bytes[101..117]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let timestamp = u64::from_be_bytes(
            bytes[117..125]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        );

        let name_len = u16::from_be_bytes(
            bytes[125..127]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        ) as usize;

        // Validate total length: 127 + name_len + 64 (sig)
        if bytes.len() != 127 + name_len + 64 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let display_name = String::from_utf8(bytes[127..127 + name_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let sig_start = 127 + name_len;
        let signature: [u8; 64] = bytes[sig_start..sig_start + 64]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let qr = ExchangeQR {
            version,
            public_key,
            exchange_key,
            exchange_token,
            audio_challenge,
            timestamp,
            display_name,
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
}

/// Maximum allowed clock drift in seconds between local time and QR timestamp.
const MAX_CLOCK_DRIFT_SECONDS: u64 = 30;

/// Checks whether the local clock and the QR timestamp are within an
/// acceptable drift window.
///
/// Returns `Ok(())` if the absolute difference is at most
/// [`MAX_CLOCK_DRIFT_SECONDS`] (30 seconds). Otherwise returns
/// `ExchangeError::ClockDrift` with the signed delta.
pub fn check_clock_drift(qr_timestamp: u64) -> Result<(), ExchangeError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    let drift = (now as i64) - (qr_timestamp as i64);

    if drift.unsigned_abs() > MAX_CLOCK_DRIFT_SECONDS {
        return Err(ExchangeError::ClockDrift(drift));
    }

    Ok(())
}

// INLINE_TEST_REQUIRED: Tests private PROTOCOL_VERSION constant and version field
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_generation() {
        let identity = Identity::create("Alice");
        let ephemeral = X3DHKeyPair::generate();
        let qr = ExchangeQR::generate(&identity, &ephemeral);

        assert_eq!(qr.version, PROTOCOL_VERSION);
        assert_eq!(qr.public_key(), identity.signing_public_key());
        assert_eq!(qr.exchange_key(), ephemeral.public_key());
        assert_eq!(qr.display_name(), "Alice");
    }

    #[test]
    fn test_qr_signature_valid() {
        let identity = Identity::create("Alice");
        let ephemeral = X3DHKeyPair::generate();
        let qr = ExchangeQR::generate(&identity, &ephemeral);

        assert!(qr.verify_signature());
    }

    #[test]
    fn test_qr_not_expired_initially() {
        let identity = Identity::create("Alice");
        let ephemeral = X3DHKeyPair::generate();
        let qr = ExchangeQR::generate(&identity, &ephemeral);

        assert!(!qr.is_expired());
    }

    #[test]
    fn test_qr_roundtrip_with_display_name() {
        let identity = Identity::create("Alice");
        let ephemeral = X3DHKeyPair::generate();
        let qr = ExchangeQR::generate(&identity, &ephemeral);

        let data_string = qr.to_data_string();
        let parsed = ExchangeQR::from_data_string(&data_string).unwrap();

        assert_eq!(parsed.display_name(), "Alice");
        assert_eq!(parsed.public_key(), qr.public_key());
        assert_eq!(parsed.exchange_key(), qr.exchange_key());
        assert_eq!(parsed.timestamp(), qr.timestamp());
        assert!(parsed.verify_signature());
    }

    #[test]
    fn test_qr_roundtrip_unicode_name() {
        let identity = Identity::create("Müller 日本語");
        let ephemeral = X3DHKeyPair::generate();
        let qr = ExchangeQR::generate(&identity, &ephemeral);

        let data_string = qr.to_data_string();
        let parsed = ExchangeQR::from_data_string(&data_string).unwrap();

        assert_eq!(parsed.display_name(), "Müller 日本語");
        assert!(parsed.verify_signature());
    }

    #[test]
    fn test_qr_roundtrip_empty_name() {
        let identity = Identity::create("");
        let ephemeral = X3DHKeyPair::generate();
        let qr = ExchangeQR::generate(&identity, &ephemeral);

        let data_string = qr.to_data_string();
        let parsed = ExchangeQR::from_data_string(&data_string).unwrap();

        assert_eq!(parsed.display_name(), "");
        assert!(parsed.verify_signature());
    }
}
