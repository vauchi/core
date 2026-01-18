//! QR Code Exchange Protocol
//!
//! Handles generation and parsing of exchange QR codes.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::time::{SystemTime, UNIX_EPOCH};

use super::ExchangeError;
use crate::crypto::{PublicKey, Signature};
use crate::identity::Identity;

/// Protocol version for QR codes.
/// v1: Original format (signing key only)
/// v2: Added X25519 exchange key for X3DH
const PROTOCOL_VERSION: u8 = 2;

/// QR code expiration time in seconds (5 minutes).
const QR_EXPIRY_SECONDS: u64 = 300;

/// QR code magic bytes to identify Vauchi QR codes.
const MAGIC: &[u8; 4] = b"WBEX";

/// Exchange QR code data structure.
///
/// Contains all information needed to initiate a contact exchange.
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
    /// Signature over the above fields
    signature: [u8; 64],
}

impl ExchangeQR {
    /// Generates a new exchange QR code for the given identity.
    pub fn generate(identity: &Identity) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self::generate_with_timestamp(identity, timestamp)
    }

    /// Generates a QR code with a specific timestamp (for testing).
    pub fn generate_with_timestamp(identity: &Identity, timestamp: u64) -> Self {
        use ring::rand::SystemRandom;

        let rng = SystemRandom::new();

        // Generate random exchange token
        let exchange_token = ring::rand::generate::<[u8; 32]>(&rng)
            .expect("RNG should not fail")
            .expose();

        // Generate random audio challenge seed
        let audio_challenge = ring::rand::generate::<[u8; 16]>(&rng)
            .expect("RNG should not fail")
            .expose();

        let public_key = *identity.signing_public_key();

        // Get X25519 exchange key for X3DH
        let exchange_key: [u8; 32] = identity
            .exchange_public_key()
            .try_into()
            .expect("Exchange key should be 32 bytes");

        // Create message to sign (all fields except signature)
        let mut message = Vec::new();
        message.push(PROTOCOL_VERSION);
        message.extend_from_slice(&public_key);
        message.extend_from_slice(&exchange_key);
        message.extend_from_slice(&exchange_token);
        message.extend_from_slice(&audio_challenge);
        message.extend_from_slice(&timestamp.to_be_bytes());

        // Sign the message
        let signature = identity.sign(&message);

        ExchangeQR {
            version: PROTOCOL_VERSION,
            public_key,
            exchange_key,
            exchange_token,
            audio_challenge,
            timestamp,
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
        // Reconstruct the signed message
        let mut message = Vec::new();
        message.push(self.version);
        message.extend_from_slice(&self.public_key);
        message.extend_from_slice(&self.exchange_key);
        message.extend_from_slice(&self.exchange_token);
        message.extend_from_slice(&self.audio_challenge);
        message.extend_from_slice(&self.timestamp.to_be_bytes());

        // Create public key for verification
        let public_key = PublicKey::from_bytes(self.public_key);
        let signature = Signature::from_bytes(self.signature);

        public_key.verify(&message, &signature)
    }

    /// Encodes the QR data to a string for embedding in QR code.
    pub fn to_data_string(&self) -> String {
        // Format: base64(MAGIC || version || public_key || exchange_key || token || challenge || timestamp || signature)
        let mut data = Vec::new();
        data.extend_from_slice(MAGIC);
        data.push(self.version);
        data.extend_from_slice(&self.public_key);
        data.extend_from_slice(&self.exchange_key);
        data.extend_from_slice(&self.exchange_token);
        data.extend_from_slice(&self.audio_challenge);
        data.extend_from_slice(&self.timestamp.to_be_bytes());
        data.extend_from_slice(&self.signature);

        BASE64.encode(&data)
    }

    /// Parses QR data from a scanned string.
    pub fn from_data_string(data: &str) -> Result<Self, ExchangeError> {
        let bytes = BASE64
            .decode(data)
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        // Check minimum length for v2 format
        // MAGIC(4) + version(1) + pubkey(32) + exchange_key(32) + token(32) + challenge(16) + timestamp(8) + sig(64) = 189
        if bytes.len() < 189 {
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

        let signature: [u8; 64] = bytes[125..189]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let qr = ExchangeQR {
            version,
            public_key,
            exchange_key,
            exchange_token,
            audio_challenge,
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
}

// INLINE_TEST_REQUIRED: Tests private PROTOCOL_VERSION constant and version field
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_generation() {
        let identity = Identity::create("Alice");
        let qr = ExchangeQR::generate(&identity);

        assert_eq!(qr.version, PROTOCOL_VERSION);
        assert_eq!(qr.public_key(), identity.signing_public_key());
    }

    #[test]
    fn test_qr_signature_valid() {
        let identity = Identity::create("Alice");
        let qr = ExchangeQR::generate(&identity);

        assert!(qr.verify_signature());
    }

    #[test]
    fn test_qr_not_expired_initially() {
        let identity = Identity::create("Alice");
        let qr = ExchangeQR::generate(&identity);

        assert!(!qr.is_expired());
    }
}
