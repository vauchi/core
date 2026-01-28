// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! NFC Tag Exchange
//!
//! NFC tag-based contact exchange using relay mailbox approach.
//! Supports both open and password-protected tags.
//!
//! Feature file: features/contact_exchange.feature @nfc

use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use thiserror::Error;

use crate::crypto::{PublicKey, Signature, SigningKeyPair};
use crate::exchange::X3DHKeyPair;

// Serde helpers for fixed-size byte arrays
mod hex_array_12 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S>(bytes: &[u8; 12], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 12], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length"))
    }
}

mod hex_array_16 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S>(bytes: &[u8; 16], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 16], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length"))
    }
}

pub mod hex_array_32 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length"))
    }
}

mod hex_array_64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length"))
    }
}

mod option_hex_array_32 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S>(bytes: &Option<[u8; 32]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match bytes {
            Some(b) => serializer.serialize_some(&hex::encode(b)),
            None => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<[u8; 32]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
                let arr: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| serde::de::Error::custom("invalid length"))?;
                Ok(Some(arr))
            }
            None => Ok(None),
        }
    }
}

mod hex_vec {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// NFC-specific errors
#[derive(Error, Debug)]
pub enum NfcError {
    #[error("Invalid magic bytes")]
    InvalidMagic,

    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u8),

    #[error("Invalid payload length")]
    InvalidLength,

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Password verification failed")]
    PasswordVerificationFailed,

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Decryption error: {0}")]
    DecryptionError(String),

    #[error("Crypto error: {0}")]
    CryptoError(String),
}

/// NFC tag operation mode
#[derive(Debug, Clone)]
pub enum NfcTagMode {
    /// Open tag - anyone can scan and send introduction
    Open,
    /// Password-protected - requires password to send introduction
    Protected { password: String },
}

/// Result of creating an NFC tag.
///
/// Contains both the payload (to write to the tag) and the exchange keypair
/// (which must be stored securely by the tag owner for decryption).
pub struct NfcTagCreationResult {
    /// The payload to write to the NFC tag
    payload: ParsedNfcPayload,
    /// The exchange keypair (private key needed for decryption)
    exchange_keypair: X3DHKeyPair,
}

impl NfcTagCreationResult {
    /// Get the payload to write to the NFC tag.
    pub fn payload(&self) -> &ParsedNfcPayload {
        &self.payload
    }

    /// Get the exchange keypair (must be stored securely for decryption).
    pub fn exchange_keypair(&self) -> &X3DHKeyPair {
        &self.exchange_keypair
    }

    /// Consume and return both parts.
    pub fn into_parts(self) -> (ParsedNfcPayload, X3DHKeyPair) {
        (self.payload, self.exchange_keypair)
    }
}

/// Open NFC tag payload (magic: "VBMB")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfcTagPayload {
    /// Signing public key (identity)
    #[serde(with = "hex_array_32")]
    pub signing_key: [u8; 32],
    /// Exchange public key (X25519)
    #[serde(with = "hex_array_32")]
    pub exchange_key: [u8; 32],
    /// Relay URL
    pub relay_url: String,
    /// Mailbox identifier
    #[serde(with = "hex_array_32")]
    pub mailbox_id: [u8; 32],
    /// Signature over all fields
    #[serde(with = "hex_array_64")]
    pub signature: [u8; 64],
}

/// Password-protected NFC tag payload (magic: "VBNP")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedNfcTagPayload {
    /// Signing public key (identity)
    #[serde(with = "hex_array_32")]
    pub signing_key: [u8; 32],
    /// Exchange public key (X25519)
    #[serde(with = "hex_array_32")]
    pub exchange_key: [u8; 32],
    /// Relay URL
    pub relay_url: String,
    /// Mailbox identifier
    #[serde(with = "hex_array_32")]
    pub mailbox_id: [u8; 32],
    /// Password salt (16 bytes)
    #[serde(with = "hex_array_16")]
    pub password_salt: [u8; 16],
    /// Password verifier (PBKDF2 hash)
    #[serde(with = "hex_array_32")]
    pub password_verifier: [u8; 32],
    /// Signature over all fields
    #[serde(with = "hex_array_64")]
    pub signature: [u8; 64],
}

/// Unified NFC payload that can be either open or protected
#[derive(Debug, Clone)]
pub enum ParsedNfcPayload {
    Open(NfcTagPayload),
    Protected(ProtectedNfcTagPayload),
}

impl ParsedNfcPayload {
    /// Get the magic bytes for this payload type
    pub fn magic(&self) -> &[u8; 4] {
        match self {
            ParsedNfcPayload::Open(_) => b"VBMB",
            ParsedNfcPayload::Protected(_) => b"VBNP",
        }
    }

    /// Get the version
    pub fn version(&self) -> u8 {
        1
    }

    /// Check if this is a password-protected payload
    pub fn is_password_protected(&self) -> bool {
        matches!(self, ParsedNfcPayload::Protected(_))
    }

    /// Verify password (returns true for open payloads)
    pub fn verify_password(&self, password: &str) -> bool {
        match self {
            ParsedNfcPayload::Open(_) => true,
            ParsedNfcPayload::Protected(p) => {
                let mut derived = [0u8; 32];
                pbkdf2::derive(
                    pbkdf2::PBKDF2_HMAC_SHA256,
                    NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
                    &p.password_salt,
                    password.as_bytes(),
                    &mut derived,
                );
                derived == p.password_verifier
            }
        }
    }

    /// Verify signature
    pub fn verify_signature(&self, public_key: &PublicKey) -> bool {
        let message = self.signable_bytes();
        let signature = self.signature_bytes();

        // Create signature object and verify
        let sig = Signature::from_bytes(signature);
        public_key.verify(&message, &sig)
    }

    /// Get the relay URL
    pub fn relay_url(&self) -> &str {
        match self {
            ParsedNfcPayload::Open(p) => &p.relay_url,
            ParsedNfcPayload::Protected(p) => &p.relay_url,
        }
    }

    /// Get the mailbox ID
    pub fn mailbox_id(&self) -> &[u8; 32] {
        match self {
            ParsedNfcPayload::Open(p) => &p.mailbox_id,
            ParsedNfcPayload::Protected(p) => &p.mailbox_id,
        }
    }

    /// Get the signing key bytes
    pub fn signing_key(&self) -> &[u8; 32] {
        match self {
            ParsedNfcPayload::Open(p) => &p.signing_key,
            ParsedNfcPayload::Protected(p) => &p.signing_key,
        }
    }

    /// Get the exchange key bytes
    pub fn exchange_key(&self) -> &[u8; 32] {
        match self {
            ParsedNfcPayload::Open(p) => &p.exchange_key,
            ParsedNfcPayload::Protected(p) => &p.exchange_key,
        }
    }

    /// Get the password salt (only for protected tags).
    pub fn password_salt(&self) -> Option<&[u8; 16]> {
        match self {
            ParsedNfcPayload::Open(_) => None,
            ParsedNfcPayload::Protected(p) => Some(&p.password_salt),
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Magic (4 bytes)
        bytes.extend_from_slice(self.magic());

        // Version (1 byte)
        bytes.push(self.version());

        match self {
            ParsedNfcPayload::Open(p) => {
                bytes.extend_from_slice(&p.signing_key);
                bytes.extend_from_slice(&p.exchange_key);
                // URL length (2 bytes) + URL
                let url_bytes = p.relay_url.as_bytes();
                bytes.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
                bytes.extend_from_slice(url_bytes);
                bytes.extend_from_slice(&p.mailbox_id);
                bytes.extend_from_slice(&p.signature);
            }
            ParsedNfcPayload::Protected(p) => {
                bytes.extend_from_slice(&p.signing_key);
                bytes.extend_from_slice(&p.exchange_key);
                // URL length (2 bytes) + URL
                let url_bytes = p.relay_url.as_bytes();
                bytes.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
                bytes.extend_from_slice(url_bytes);
                bytes.extend_from_slice(&p.mailbox_id);
                bytes.extend_from_slice(&p.password_salt);
                bytes.extend_from_slice(&p.password_verifier);
                bytes.extend_from_slice(&p.signature);
            }
        }

        bytes
    }

    /// Get bytes to sign (everything except signature)
    fn signable_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Magic (4 bytes)
        bytes.extend_from_slice(self.magic());

        // Version (1 byte)
        bytes.push(self.version());

        match self {
            ParsedNfcPayload::Open(p) => {
                bytes.extend_from_slice(&p.signing_key);
                bytes.extend_from_slice(&p.exchange_key);
                let url_bytes = p.relay_url.as_bytes();
                bytes.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
                bytes.extend_from_slice(url_bytes);
                bytes.extend_from_slice(&p.mailbox_id);
            }
            ParsedNfcPayload::Protected(p) => {
                bytes.extend_from_slice(&p.signing_key);
                bytes.extend_from_slice(&p.exchange_key);
                let url_bytes = p.relay_url.as_bytes();
                bytes.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
                bytes.extend_from_slice(url_bytes);
                bytes.extend_from_slice(&p.mailbox_id);
                bytes.extend_from_slice(&p.password_salt);
                bytes.extend_from_slice(&p.password_verifier);
            }
        }

        bytes
    }

    /// Get the signature bytes
    fn signature_bytes(&self) -> [u8; 64] {
        match self {
            ParsedNfcPayload::Open(p) => p.signature,
            ParsedNfcPayload::Protected(p) => p.signature,
        }
    }
}

/// Number of PBKDF2 iterations for password verification
/// High enough to be slow for brute force, fast enough for UX
const PBKDF2_ITERATIONS: u32 = 100_000;

/// Create an NFC tag with payload and exchange keypair.
///
/// Returns both the payload (to write to the NFC tag) and the exchange keypair
/// (which must be stored securely by the tag owner for decrypting introductions).
///
/// # Example
///
/// ```ignore
/// let result = create_nfc_tag(&keypair, "wss://relay.app", &mailbox_id, NfcTagMode::Open)?;
///
/// // Write payload to NFC tag
/// write_to_tag(result.payload().to_bytes());
///
/// // Store exchange keypair securely for later decryption
/// store_securely(result.exchange_keypair().secret_bytes());
/// ```
pub fn create_nfc_tag(
    keypair: &SigningKeyPair,
    relay_url: &str,
    mailbox_id: &[u8; 32],
    mode: NfcTagMode,
) -> Result<NfcTagCreationResult, NfcError> {
    let rng = SystemRandom::new();

    // Generate exchange keypair for X3DH
    let exchange_keypair = X3DHKeyPair::generate();

    let payload = match mode {
        NfcTagMode::Open => {
            let mut payload = NfcTagPayload {
                signing_key: *keypair.public_key().as_bytes(),
                exchange_key: *exchange_keypair.public_key(),
                relay_url: relay_url.to_string(),
                mailbox_id: *mailbox_id,
                signature: [0u8; 64],
            };

            let signable = create_signable_bytes_open(&payload);
            let signature = keypair.sign(&signable);
            payload.signature = *signature.as_bytes();

            ParsedNfcPayload::Open(payload)
        }
        NfcTagMode::Protected { password } => {
            let mut salt = [0u8; 16];
            rng.fill(&mut salt)
                .map_err(|_| NfcError::CryptoError("Failed to generate salt".into()))?;

            let mut verifier = [0u8; 32];
            pbkdf2::derive(
                pbkdf2::PBKDF2_HMAC_SHA256,
                NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
                &salt,
                password.as_bytes(),
                &mut verifier,
            );

            let mut payload = ProtectedNfcTagPayload {
                signing_key: *keypair.public_key().as_bytes(),
                exchange_key: *exchange_keypair.public_key(),
                relay_url: relay_url.to_string(),
                mailbox_id: *mailbox_id,
                password_salt: salt,
                password_verifier: verifier,
                signature: [0u8; 64],
            };

            let signable = create_signable_bytes_protected(&payload);
            let signature = keypair.sign(&signable);
            payload.signature = *signature.as_bytes();

            ParsedNfcPayload::Protected(payload)
        }
    };

    Ok(NfcTagCreationResult {
        payload,
        exchange_keypair,
    })
}

/// Parse NFC tag payload from bytes
pub fn parse_nfc_payload(bytes: &[u8]) -> Result<ParsedNfcPayload, NfcError> {
    if bytes.len() < 5 {
        return Err(NfcError::InvalidLength);
    }

    let magic = &bytes[0..4];
    let version = bytes[4];

    if version != 1 {
        return Err(NfcError::UnsupportedVersion(version));
    }

    match magic {
        b"VBMB" => parse_open_payload(&bytes[5..]),
        b"VBNP" => parse_protected_payload(&bytes[5..]),
        _ => Err(NfcError::InvalidMagic),
    }
}

fn parse_open_payload(bytes: &[u8]) -> Result<ParsedNfcPayload, NfcError> {
    // signing_key(32) + exchange_key(32) + url_len(2) + min_url(1) + mailbox(32) + sig(64)
    if bytes.len() < 32 + 32 + 2 + 1 + 32 + 64 {
        return Err(NfcError::InvalidLength);
    }

    let mut signing_key = [0u8; 32];
    signing_key.copy_from_slice(&bytes[0..32]);

    let mut exchange_key = [0u8; 32];
    exchange_key.copy_from_slice(&bytes[32..64]);

    let url_len = u16::from_be_bytes([bytes[64], bytes[65]]) as usize;
    if bytes.len() < 66 + url_len + 32 + 64 {
        return Err(NfcError::InvalidLength);
    }

    let relay_url =
        String::from_utf8(bytes[66..66 + url_len].to_vec()).map_err(|_| NfcError::InvalidLength)?;

    let offset = 66 + url_len;
    let mut mailbox_id = [0u8; 32];
    mailbox_id.copy_from_slice(&bytes[offset..offset + 32]);

    let mut signature = [0u8; 64];
    signature.copy_from_slice(&bytes[offset + 32..offset + 96]);

    Ok(ParsedNfcPayload::Open(NfcTagPayload {
        signing_key,
        exchange_key,
        relay_url,
        mailbox_id,
        signature,
    }))
}

fn parse_protected_payload(bytes: &[u8]) -> Result<ParsedNfcPayload, NfcError> {
    // signing_key(32) + exchange_key(32) + url_len(2) + min_url(1) + mailbox(32) + salt(16) + verifier(32) + sig(64)
    if bytes.len() < 32 + 32 + 2 + 1 + 32 + 16 + 32 + 64 {
        return Err(NfcError::InvalidLength);
    }

    let mut signing_key = [0u8; 32];
    signing_key.copy_from_slice(&bytes[0..32]);

    let mut exchange_key = [0u8; 32];
    exchange_key.copy_from_slice(&bytes[32..64]);

    let url_len = u16::from_be_bytes([bytes[64], bytes[65]]) as usize;
    if bytes.len() < 66 + url_len + 32 + 16 + 32 + 64 {
        return Err(NfcError::InvalidLength);
    }

    let relay_url =
        String::from_utf8(bytes[66..66 + url_len].to_vec()).map_err(|_| NfcError::InvalidLength)?;

    let offset = 66 + url_len;
    let mut mailbox_id = [0u8; 32];
    mailbox_id.copy_from_slice(&bytes[offset..offset + 32]);

    let mut password_salt = [0u8; 16];
    password_salt.copy_from_slice(&bytes[offset + 32..offset + 48]);

    let mut password_verifier = [0u8; 32];
    password_verifier.copy_from_slice(&bytes[offset + 48..offset + 80]);

    let mut signature = [0u8; 64];
    signature.copy_from_slice(&bytes[offset + 80..offset + 144]);

    Ok(ParsedNfcPayload::Protected(ProtectedNfcTagPayload {
        signing_key,
        exchange_key,
        relay_url,
        mailbox_id,
        password_salt,
        password_verifier,
        signature,
    }))
}

fn create_signable_bytes_open(payload: &NfcTagPayload) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"VBMB");
    bytes.push(1); // version
    bytes.extend_from_slice(&payload.signing_key);
    bytes.extend_from_slice(&payload.exchange_key);
    let url_bytes = payload.relay_url.as_bytes();
    bytes.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
    bytes.extend_from_slice(url_bytes);
    bytes.extend_from_slice(&payload.mailbox_id);
    bytes
}

fn create_signable_bytes_protected(payload: &ProtectedNfcTagPayload) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"VBNP");
    bytes.push(1); // version
    bytes.extend_from_slice(&payload.signing_key);
    bytes.extend_from_slice(&payload.exchange_key);
    let url_bytes = payload.relay_url.as_bytes();
    bytes.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
    bytes.extend_from_slice(url_bytes);
    bytes.extend_from_slice(&payload.mailbox_id);
    bytes.extend_from_slice(&payload.password_salt);
    bytes.extend_from_slice(&payload.password_verifier);
    bytes
}

/// Introduction message sent via relay mailbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Introduction {
    /// Sender's signing public key
    #[serde(with = "option_hex_array_32")]
    pub sender_signing_key: Option<[u8; 32]>,
    /// Sender's ephemeral X25519 public key (for X3DH)
    #[serde(with = "hex_array_32")]
    pub ephemeral_key: [u8; 32],
    /// Encrypted contact card data
    #[serde(with = "hex_vec")]
    pub ciphertext: Vec<u8>,
    /// Nonce used for encryption
    #[serde(with = "hex_array_12")]
    pub nonce: [u8; 12],
}

impl Introduction {
    /// Create an introduction for an open NFC tag
    pub fn create(
        sender_keypair: &SigningKeyPair,
        tag_payload: &ParsedNfcPayload,
        contact_card_data: &[u8],
    ) -> Result<Self, NfcError> {
        Self::create_internal(sender_keypair, tag_payload, contact_card_data, None)
    }

    /// Create an introduction with password
    pub fn create_with_password(
        sender_keypair: &SigningKeyPair,
        tag_payload: &ParsedNfcPayload,
        contact_card_data: &[u8],
        password: &str,
    ) -> Result<Self, NfcError> {
        Self::create_internal(
            sender_keypair,
            tag_payload,
            contact_card_data,
            Some(password),
        )
    }

    fn create_internal(
        sender_keypair: &SigningKeyPair,
        tag_payload: &ParsedNfcPayload,
        contact_card_data: &[u8],
        password: Option<&str>,
    ) -> Result<Self, NfcError> {
        use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
        use ring::agreement::{agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey, X25519};
        use ring::hkdf::{Salt, HKDF_SHA256};

        let rng = SystemRandom::new();

        // Generate ephemeral X25519 key for X3DH
        let ephemeral_private = EphemeralPrivateKey::generate(&X25519, &rng)
            .map_err(|_| NfcError::CryptoError("Failed to generate ephemeral key".into()))?;

        let mut ephemeral_public = [0u8; 32];
        ephemeral_public.copy_from_slice(
            ephemeral_private
                .compute_public_key()
                .map_err(|_| NfcError::CryptoError("Failed to compute public key".into()))?
                .as_ref(),
        );

        // Get tag's exchange key
        let tag_exchange_key = UnparsedPublicKey::new(&X25519, tag_payload.exchange_key());

        // Perform X25519 key agreement
        let shared_secret = agree_ephemeral(ephemeral_private, &tag_exchange_key, |key_material| {
            key_material.to_vec()
        })
        .map_err(|_| NfcError::CryptoError("Key agreement failed".into()))?;

        // Derive encryption key using HKDF
        let salt_bytes = match password {
            Some(pwd) => {
                // Include password in key derivation
                let mut pwd_derived = [0u8; 32];
                if let ParsedNfcPayload::Protected(p) = tag_payload {
                    pbkdf2::derive(
                        pbkdf2::PBKDF2_HMAC_SHA256,
                        NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
                        &p.password_salt,
                        pwd.as_bytes(),
                        &mut pwd_derived,
                    );
                }
                pwd_derived.to_vec()
            }
            None => vec![0u8; 32],
        };

        let salt = Salt::new(HKDF_SHA256, &salt_bytes);
        let prk = salt.extract(&shared_secret);
        let okm = prk
            .expand(&[b"Vauchi_NFC_Intro"], HKDF_SHA256)
            .map_err(|_| NfcError::CryptoError("HKDF expand failed".into()))?;

        let mut key_bytes = [0u8; 32];
        okm.fill(&mut key_bytes)
            .map_err(|_| NfcError::CryptoError("HKDF fill failed".into()))?;

        // Generate nonce
        let mut nonce_bytes = [0u8; 12];
        rng.fill(&mut nonce_bytes)
            .map_err(|_| NfcError::CryptoError("Failed to generate nonce".into()))?;

        // Encrypt contact card
        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
            .map_err(|_| NfcError::EncryptionError("Invalid key".into()))?;
        let key = LessSafeKey::new(unbound_key);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut ciphertext = contact_card_data.to_vec();
        key.seal_in_place_append_tag(nonce, Aad::empty(), &mut ciphertext)
            .map_err(|_| NfcError::EncryptionError("Encryption failed".into()))?;

        Ok(Introduction {
            sender_signing_key: Some(*sender_keypair.public_key().as_bytes()),
            ephemeral_key: ephemeral_public,
            ciphertext,
            nonce: nonce_bytes,
        })
    }

    /// Get the ciphertext
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    /// Get the sender's signing key if present
    pub fn sender_signing_key(&self) -> Option<&[u8; 32]> {
        self.sender_signing_key.as_ref()
    }

    /// Decrypt the introduction (called by tag owner)
    ///
    /// **Deprecated**: This method is fundamentally broken and will not work.
    /// Use `decrypt_with_exchange_key` instead, which takes the X25519 keypair
    /// from `create_nfc_tag`.
    #[deprecated(
        since = "0.2.0",
        note = "This method is broken. Use decrypt_with_exchange_key instead."
    )]
    pub fn decrypt(
        &self,
        _owner_keypair: &SigningKeyPair,
        password: Option<&str>,
    ) -> Result<Vec<u8>, NfcError> {
        use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
        use ring::agreement::{agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey, X25519};
        use ring::hkdf::{Salt, HKDF_SHA256};

        // Note: In real implementation, we'd need the owner's X25519 private key
        // which would be derived from or stored alongside the signing keypair.
        // For now, this is a simplified implementation.

        // For this test implementation, we'll derive a key from the signing keypair
        // In production, this would use proper X25519 key storage
        let rng = SystemRandom::new();

        // Recreate the shared secret using owner's exchange key
        // This is simplified - real impl would use stored X25519 private key
        let owner_private = EphemeralPrivateKey::generate(&X25519, &rng)
            .map_err(|_| NfcError::CryptoError("Key generation failed".into()))?;

        let sender_ephemeral = UnparsedPublicKey::new(&X25519, &self.ephemeral_key);

        let shared_secret = agree_ephemeral(owner_private, &sender_ephemeral, |key_material| {
            key_material.to_vec()
        })
        .map_err(|_| NfcError::CryptoError("Key agreement failed".into()))?;

        // Derive decryption key
        let salt_bytes = match password {
            Some(pwd) => {
                let mut pwd_derived = [0u8; 32];
                // Would need salt from tag - simplified for test
                let salt = [0u8; 16];
                pbkdf2::derive(
                    pbkdf2::PBKDF2_HMAC_SHA256,
                    NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
                    &salt,
                    pwd.as_bytes(),
                    &mut pwd_derived,
                );
                pwd_derived.to_vec()
            }
            None => vec![0u8; 32],
        };

        let salt = Salt::new(HKDF_SHA256, &salt_bytes);
        let prk = salt.extract(&shared_secret);
        let okm = prk
            .expand(&[b"Vauchi_NFC_Intro"], HKDF_SHA256)
            .map_err(|_| NfcError::CryptoError("HKDF expand failed".into()))?;

        let mut key_bytes = [0u8; 32];
        okm.fill(&mut key_bytes)
            .map_err(|_| NfcError::CryptoError("HKDF fill failed".into()))?;

        // Decrypt
        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
            .map_err(|_| NfcError::DecryptionError("Invalid key".into()))?;
        let key = LessSafeKey::new(unbound_key);
        let nonce = Nonce::assume_unique_for_key(self.nonce);

        let mut plaintext = self.ciphertext.clone();
        key.open_in_place(nonce, Aad::empty(), &mut plaintext)
            .map_err(|_| NfcError::DecryptionError("Decryption failed".into()))?;

        // Remove auth tag
        plaintext.truncate(plaintext.len() - 16);

        Ok(plaintext)
    }

    /// Decrypt the introduction using the tag owner's exchange keypair.
    ///
    /// This is the proper decryption method that uses the X25519 keypair
    /// that was generated when creating the NFC tag.
    ///
    /// # Arguments
    ///
    /// * `exchange_keypair` - The X25519 keypair from `create_nfc_tag`
    /// * `password_with_salt` - Optional (password, salt) tuple for protected tags.
    ///   The salt comes from the stored `ParsedNfcPayload::Protected.password_salt`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tag_result = create_nfc_tag(&keypair, url, &mailbox_id, NfcTagMode::Open)?;
    /// // ... tag_result.exchange_keypair() and tag_result.payload() are stored securely ...
    ///
    /// // Later, when receiving an introduction (open tag):
    /// let plaintext = intro.decrypt_with_exchange_key(&stored_exchange_keypair, None)?;
    ///
    /// // For protected tags:
    /// if let ParsedNfcPayload::Protected(p) = &stored_payload {
    ///     let plaintext = intro.decrypt_with_exchange_key(
    ///         &stored_exchange_keypair,
    ///         Some(("password", &p.password_salt))
    ///     )?;
    /// }
    /// ```
    pub fn decrypt_with_exchange_key(
        &self,
        exchange_keypair: &X3DHKeyPair,
        password_with_salt: Option<(&str, &[u8; 16])>,
    ) -> Result<Vec<u8>, NfcError> {
        use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
        use ring::hkdf::{Salt, HKDF_SHA256};

        // Perform X25519 key agreement: our_static_secret * their_ephemeral_public
        let shared_secret = exchange_keypair.diffie_hellman(&self.ephemeral_key);

        // Derive decryption key using HKDF
        let salt_bytes = match password_with_salt {
            Some((pwd, salt)) => {
                // Include password in key derivation with the stored salt
                let mut pwd_derived = [0u8; 32];
                pbkdf2::derive(
                    pbkdf2::PBKDF2_HMAC_SHA256,
                    NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
                    salt,
                    pwd.as_bytes(),
                    &mut pwd_derived,
                );
                pwd_derived.to_vec()
            }
            None => vec![0u8; 32],
        };

        let salt = Salt::new(HKDF_SHA256, &salt_bytes);
        let prk = salt.extract(&shared_secret);
        let okm = prk
            .expand(&[b"Vauchi_NFC_Intro"], HKDF_SHA256)
            .map_err(|_| NfcError::CryptoError("HKDF expand failed".into()))?;

        let mut key_bytes = [0u8; 32];
        okm.fill(&mut key_bytes)
            .map_err(|_| NfcError::CryptoError("HKDF fill failed".into()))?;

        // Decrypt
        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
            .map_err(|_| NfcError::DecryptionError("Invalid key".into()))?;
        let key = LessSafeKey::new(unbound_key);
        let nonce = Nonce::assume_unique_for_key(self.nonce);

        let mut plaintext = self.ciphertext.clone();
        key.open_in_place(nonce, Aad::empty(), &mut plaintext)
            .map_err(|_| NfcError::DecryptionError("Decryption failed".into()))?;

        // Remove auth tag
        plaintext.truncate(plaintext.len() - 16);

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_open_payload() {
        let keypair = SigningKeyPair::generate();
        let mailbox_id = [1u8; 32];

        let tag_result =
            create_nfc_tag(&keypair, "wss://relay.test", &mailbox_id, NfcTagMode::Open).unwrap();

        let payload = tag_result.payload();
        assert!(!payload.is_password_protected());
        assert_eq!(payload.relay_url(), "wss://relay.test");

        // Exchange keypair should match payload's exchange key
        assert_eq!(
            tag_result.exchange_keypair().public_key(),
            payload.exchange_key()
        );
    }

    #[test]
    fn test_create_protected_payload() {
        let keypair = SigningKeyPair::generate();
        let mailbox_id = [1u8; 32];

        let tag_result = create_nfc_tag(
            &keypair,
            "wss://relay.test",
            &mailbox_id,
            NfcTagMode::Protected {
                password: "test".to_string(),
            },
        )
        .unwrap();

        let payload = tag_result.payload();
        assert!(payload.is_password_protected());
        assert!(payload.verify_password("test"));
        assert!(!payload.verify_password("wrong"));
    }

    #[test]
    fn test_payload_roundtrip() {
        let keypair = SigningKeyPair::generate();
        let mailbox_id = [42u8; 32];

        let tag_result = create_nfc_tag(
            &keypair,
            "wss://relay.example.com",
            &mailbox_id,
            NfcTagMode::Open,
        )
        .unwrap();

        let bytes = tag_result.payload().to_bytes();
        let parsed = parse_nfc_payload(&bytes).unwrap();

        assert_eq!(parsed.relay_url(), "wss://relay.example.com");
        assert_eq!(parsed.mailbox_id(), &mailbox_id);
    }
}
