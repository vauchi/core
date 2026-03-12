// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! QR Code Exchange Protocol
//!
//! Handles generation and parsing of exchange QR codes.
//!
//! ## Wire format (v3)
//!
//! ```text
//! MAGIC(4) || version(1) || pubkey(32) || exchange_key(32) || token(32)
//! || challenge(16) || timestamp(8) || name_len(2) || name(N)
//! || flags(1) || [relay_url_len(2) || relay_url(M)]
//! || [relay_noise_pubkey(32)]
//! || signature(64)
//! ```
//!
//! Flags byte (bitfield):
//! - Bit 0: has_relay_url — if set, relay_url_len + relay_url follow
//! - Bit 1: has_relay_noise_pubkey — if set, 32-byte pubkey follows relay_url (or flags if no URL)
//! - Bits 2-7: reserved (must be zero)

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::time::{SystemTime, UNIX_EPOCH};

use super::x3dh::X3DHKeyPair;
use super::ExchangeError;
use crate::crypto::{PublicKey, Signature};
use crate::identity::Identity;

/// Protocol version for QR codes.
/// v1: Original format (signing key only)
/// v2: Added X25519 exchange key for X3DH
/// v3: Added relay URL + Noise NK pubkey for per-contact routing
const PROTOCOL_VERSION: u8 = 3;

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

/// Flag: relay URL is present after display name
const FLAG_HAS_RELAY_URL: u8 = 0x01;
/// Flag: relay Noise NK pubkey is present
const FLAG_HAS_RELAY_NOISE_PUBKEY: u8 = 0x02;

/// Exchange QR code data structure.
///
/// Contains all information needed to initiate a contact exchange,
/// including the displayer's display name and optional relay metadata
/// for per-contact relay routing.
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
    /// Relay URL for per-contact routing (learned during exchange)
    relay_url: Option<String>,
    /// Relay's Noise NK public key (pinned during exchange, eliminates TOFU)
    relay_noise_pubkey: Option<[u8; 32]>,
    /// Signature over the above fields (including relay metadata)
    signature: [u8; 64],
}

impl ExchangeQR {
    /// Generates a new exchange QR code with a fresh ephemeral X25519 key.
    ///
    /// The exchange key in the QR is the provided ephemeral key, not the
    /// identity's static X3DH key. This gives full forward secrecy.
    pub fn generate(identity: &Identity, ephemeral: &X3DHKeyPair) -> Self {
        Self::generate_with_relay(identity, ephemeral, None, None)
    }

    /// Generates a QR code with optional relay metadata.
    ///
    /// When relay_url and/or relay_noise_pubkey are provided, they are
    /// included in the QR code and signed. The recipient learns the
    /// sender's relay during in-person exchange.
    pub fn generate_with_relay(
        identity: &Identity,
        ephemeral: &X3DHKeyPair,
        relay_url: Option<String>,
        relay_noise_pubkey: Option<[u8; 32]>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self::generate_with_relay_and_timestamp(
            identity,
            ephemeral,
            timestamp,
            relay_url,
            relay_noise_pubkey,
        )
    }

    /// Generates a QR code with a specific timestamp (for testing).
    pub fn generate_with_timestamp(
        identity: &Identity,
        ephemeral: &X3DHKeyPair,
        timestamp: u64,
    ) -> Self {
        Self::generate_with_relay_and_timestamp(identity, ephemeral, timestamp, None, None)
    }

    /// Full constructor with all parameters.
    fn generate_with_relay_and_timestamp(
        identity: &Identity,
        ephemeral: &X3DHKeyPair,
        timestamp: u64,
        relay_url: Option<String>,
        relay_noise_pubkey: Option<[u8; 32]>,
    ) -> Self {
        use aws_lc_rs::rand::SystemRandom;

        let rng = SystemRandom::new();

        let exchange_token = aws_lc_rs::rand::generate::<[u8; 32]>(&rng)
            .expect("RNG should not fail")
            .expose();

        let audio_challenge = aws_lc_rs::rand::generate::<[u8; 16]>(&rng)
            .expect("RNG should not fail")
            .expose();

        let public_key = *identity.signing_public_key();
        let exchange_key: [u8; 32] = *ephemeral.public_key();
        let display_name = identity.display_name().to_string();

        let message = build_signed_message(
            PROTOCOL_VERSION,
            &public_key,
            &exchange_key,
            &exchange_token,
            &audio_challenge,
            timestamp,
            &display_name,
            relay_url.as_deref(),
            relay_noise_pubkey.as_ref(),
        );

        let signature = identity.sign(&message);

        ExchangeQR {
            version: PROTOCOL_VERSION,
            public_key,
            exchange_key,
            exchange_token,
            audio_challenge,
            timestamp,
            display_name,
            relay_url,
            relay_noise_pubkey,
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

    /// Returns the relay URL, if present.
    pub fn relay_url(&self) -> Option<&str> {
        self.relay_url.as_deref()
    }

    /// Returns the relay Noise NK public key, if present.
    pub fn relay_noise_pubkey(&self) -> Option<&[u8; 32]> {
        self.relay_noise_pubkey.as_ref()
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
        let message = build_signed_message(
            self.version,
            &self.public_key,
            &self.exchange_key,
            &self.exchange_token,
            &self.audio_challenge,
            self.timestamp,
            &self.display_name,
            self.relay_url.as_deref(),
            self.relay_noise_pubkey.as_ref(),
        );

        let public_key = PublicKey::from_bytes(self.public_key);
        let signature = Signature::from_bytes(self.signature);

        public_key.verify(&message, &signature)
    }

    /// Encodes the QR data to a string for embedding in QR code.
    pub fn to_data_string(&self) -> String {
        let name_bytes = self.display_name.as_bytes();
        let name_len = name_bytes.len() as u16;

        let flags = build_flags(self.relay_url.as_deref(), self.relay_noise_pubkey.as_ref());

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
        // v3: flags + optional relay fields
        data.push(flags);
        if let Some(ref url) = self.relay_url {
            let url_bytes = url.as_bytes();
            let url_len = url_bytes.len() as u16;
            data.extend_from_slice(&url_len.to_be_bytes());
            data.extend_from_slice(url_bytes);
        }
        if let Some(ref pubkey) = self.relay_noise_pubkey {
            data.extend_from_slice(pubkey);
        }
        data.extend_from_slice(&self.signature);

        BASE64.encode(&data)
    }

    /// Parses QR data from a scanned string.
    pub fn from_data_string(data: &str) -> Result<Self, ExchangeError> {
        let bytes = BASE64
            .decode(data)
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        // Minimum: MAGIC(4) + version(1) + pubkey(32) + exchange_key(32)
        //   + token(32) + challenge(16) + timestamp(8) + name_len(2) + flags(1) + sig(64) = 192
        if bytes.len() < 192 {
            return Err(ExchangeError::InvalidQRFormat);
        }

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

        // Bounds check for name
        let name_end = 127 + name_len;
        if bytes.len() < name_end + 1 {
            // +1 for flags byte
            return Err(ExchangeError::InvalidQRFormat);
        }

        let display_name = String::from_utf8(bytes[127..name_end].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        // Parse flags byte
        let flags = bytes[name_end];
        let mut cursor = name_end + 1;

        // Parse optional relay URL
        let relay_url = if flags & FLAG_HAS_RELAY_URL != 0 {
            if bytes.len() < cursor + 2 {
                return Err(ExchangeError::InvalidQRFormat);
            }
            let url_len = u16::from_be_bytes(
                bytes[cursor..cursor + 2]
                    .try_into()
                    .map_err(|_| ExchangeError::InvalidQRFormat)?,
            ) as usize;
            cursor += 2;
            if bytes.len() < cursor + url_len {
                return Err(ExchangeError::InvalidQRFormat);
            }
            let url = String::from_utf8(bytes[cursor..cursor + url_len].to_vec())
                .map_err(|_| ExchangeError::InvalidQRFormat)?;
            cursor += url_len;
            Some(url)
        } else {
            None
        };

        // Parse optional relay Noise pubkey
        let relay_noise_pubkey = if flags & FLAG_HAS_RELAY_NOISE_PUBKEY != 0 {
            if bytes.len() < cursor + 32 {
                return Err(ExchangeError::InvalidQRFormat);
            }
            let pubkey: [u8; 32] = bytes[cursor..cursor + 32]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?;
            cursor += 32;
            Some(pubkey)
        } else {
            None
        };

        // Remaining bytes must be the signature (64 bytes)
        if bytes.len() != cursor + 64 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let signature: [u8; 64] = bytes[cursor..cursor + 64]
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
            relay_url,
            relay_noise_pubkey,
            signature,
        };

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

/// Builds the message bytes that are signed.
#[allow(clippy::too_many_arguments)]
fn build_signed_message(
    version: u8,
    public_key: &[u8; 32],
    exchange_key: &[u8; 32],
    exchange_token: &[u8; 32],
    audio_challenge: &[u8; 16],
    timestamp: u64,
    display_name: &str,
    relay_url: Option<&str>,
    relay_noise_pubkey: Option<&[u8; 32]>,
) -> Vec<u8> {
    let name_bytes = display_name.as_bytes();
    let name_len = name_bytes.len() as u16;
    let flags = build_flags(relay_url, relay_noise_pubkey);

    let mut message = Vec::new();
    message.push(version);
    message.extend_from_slice(public_key);
    message.extend_from_slice(exchange_key);
    message.extend_from_slice(exchange_token);
    message.extend_from_slice(audio_challenge);
    message.extend_from_slice(&timestamp.to_be_bytes());
    message.extend_from_slice(&name_len.to_be_bytes());
    message.extend_from_slice(name_bytes);
    // v3: flags + optional relay fields are part of signed data
    message.push(flags);
    if let Some(url) = relay_url {
        let url_bytes = url.as_bytes();
        let url_len = url_bytes.len() as u16;
        message.extend_from_slice(&url_len.to_be_bytes());
        message.extend_from_slice(url_bytes);
    }
    if let Some(pubkey) = relay_noise_pubkey {
        message.extend_from_slice(pubkey);
    }
    message
}

/// Builds the flags byte from optional relay fields.
fn build_flags(relay_url: Option<&str>, relay_noise_pubkey: Option<&[u8; 32]>) -> u8 {
    let mut flags = 0u8;
    if relay_url.is_some() {
        flags |= FLAG_HAS_RELAY_URL;
    }
    if relay_noise_pubkey.is_some() {
        flags |= FLAG_HAS_RELAY_NOISE_PUBKEY;
    }
    flags
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

// INLINE_TEST_REQUIRED: Tests private PROTOCOL_VERSION constant, flags, and version field
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version_is_v3() {
        assert_eq!(PROTOCOL_VERSION, 3);
    }

    #[test]
    fn test_qr_generation() {
        let identity = Identity::create("Alice");
        let ephemeral = X3DHKeyPair::generate();
        let qr = ExchangeQR::generate(&identity, &ephemeral);

        assert_eq!(qr.version, PROTOCOL_VERSION);
        assert_eq!(qr.public_key(), identity.signing_public_key());
        assert_eq!(qr.exchange_key(), ephemeral.public_key());
        assert_eq!(qr.display_name(), "Alice");
        assert!(qr.relay_url().is_none());
        assert!(qr.relay_noise_pubkey().is_none());
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

    #[test]
    fn test_flags_none() {
        assert_eq!(build_flags(None, None), 0x00);
    }

    #[test]
    fn test_flags_url_only() {
        assert_eq!(build_flags(Some("wss://relay.example.com"), None), 0x01);
    }

    #[test]
    fn test_flags_pubkey_only() {
        assert_eq!(build_flags(None, Some(&[0u8; 32])), 0x02);
    }

    #[test]
    fn test_flags_both() {
        assert_eq!(
            build_flags(Some("wss://relay.example.com"), Some(&[0u8; 32])),
            0x03
        );
    }

    #[test]
    fn test_v3_relay_fields_in_signature() {
        // Verify that changing relay fields invalidates the signature
        let identity = Identity::create("Test");
        let ephemeral = X3DHKeyPair::generate();

        let qr_with_relay = ExchangeQR::generate_with_relay(
            &identity,
            &ephemeral,
            Some("wss://relay.example.com".to_string()),
            Some([1u8; 32]),
        );

        // Manually tamper with relay URL
        let mut tampered = qr_with_relay.clone();
        tampered.relay_url = Some("wss://evil.example.com".to_string());
        assert!(
            !tampered.verify_signature(),
            "Tampered relay URL must invalidate signature"
        );

        // Manually tamper with noise pubkey
        let mut tampered2 = qr_with_relay.clone();
        tampered2.relay_noise_pubkey = Some([2u8; 32]);
        assert!(
            !tampered2.verify_signature(),
            "Tampered Noise pubkey must invalidate signature"
        );
    }
}
