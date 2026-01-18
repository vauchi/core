//! Encrypted Exchange Message
//!
//! Provides end-to-end encrypted exchange messages for contact exchange.
//! Uses X3DH to derive a shared secret, then encrypts the identity key
//! and display name so the relay cannot read them.

use serde::{Deserialize, Serialize};

use super::{ExchangeError, X3DHKeyPair, X3DH};
use crate::crypto::{decrypt, encrypt, SymmetricKey};

/// Serde helper for 32-byte arrays (base64 encoded).
mod bytes_array_32 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length for 32-byte array"))
    }
}

/// An encrypted exchange message for secure contact card exchange.
///
/// The ephemeral public key is sent in plaintext (required for X3DH),
/// while the identity key and display name are encrypted with the
/// X3DH-derived shared secret.
///
/// Wire format:
/// - ephemeral_public_key: 32 bytes (plaintext, needed for key agreement)
/// - ciphertext: variable (encrypted identity key + display name)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedExchangeMessage {
    /// Ephemeral public key for X3DH (plaintext, 32 bytes).
    /// The recipient uses this to derive the same shared secret.
    #[serde(with = "bytes_array_32")]
    pub ephemeral_public_key: [u8; 32],

    /// Encrypted payload containing identity key and display name.
    /// Format: AES-256-GCM(nonce || ciphertext || tag)
    pub ciphertext: Vec<u8>,
}

/// Internal payload structure (encrypted inside ciphertext).
#[derive(Debug, Serialize, Deserialize)]
struct ExchangePayload {
    /// Sender's signing/identity public key (32 bytes).
    #[serde(with = "bytes_array_32")]
    identity_key: [u8; 32],
    /// Sender's X3DH public key (for recipient to send encrypted responses).
    #[serde(with = "bytes_array_32")]
    exchange_key: [u8; 32],
    /// Sender's display name.
    display_name: String,
}

/// Decrypted exchange message payload.
#[derive(Debug, Clone)]
pub struct DecryptedExchangePayload {
    /// Sender's signing/identity public key.
    pub identity_key: [u8; 32],
    /// Sender's X3DH public key (for sending encrypted responses).
    pub exchange_key: [u8; 32],
    /// Sender's display name.
    pub display_name: String,
}

impl EncryptedExchangeMessage {
    /// Creates an encrypted exchange message.
    ///
    /// Uses X3DH to derive a shared secret with the recipient's public key,
    /// then encrypts the sender's identity key, exchange key, and display name.
    ///
    /// # Arguments
    /// * `our_keys` - Our X3DH keypair (used to generate ephemeral and included in payload)
    /// * `their_public` - Recipient's X3DH public key
    /// * `our_identity_key` - Our signing/identity public key to share
    /// * `our_display_name` - Our display name to share
    ///
    /// # Returns
    /// A tuple of (EncryptedExchangeMessage, shared_secret) where the shared
    /// secret can be used for subsequent communication.
    pub fn create(
        our_keys: &X3DHKeyPair,
        their_public: &[u8; 32],
        our_identity_key: &[u8; 32],
        our_display_name: &str,
    ) -> Result<(Self, SymmetricKey), ExchangeError> {
        // Perform X3DH key agreement to get shared secret and ephemeral key
        let (shared_secret, ephemeral_public_key) = X3DH::initiate(our_keys, their_public)?;

        // Create the payload to encrypt (includes our X3DH public key for responses)
        let payload = ExchangePayload {
            identity_key: *our_identity_key,
            exchange_key: *our_keys.public_key(),
            display_name: our_display_name.to_string(),
        };

        // Serialize payload to JSON
        let payload_bytes =
            serde_json::to_vec(&payload).map_err(|_| ExchangeError::SerializationFailed)?;

        // Encrypt with the shared secret
        let ciphertext =
            encrypt(&shared_secret, &payload_bytes).map_err(|_| ExchangeError::CryptoError)?;

        Ok((
            EncryptedExchangeMessage {
                ephemeral_public_key,
                ciphertext,
            },
            shared_secret,
        ))
    }

    /// Decrypts an exchange message using our X3DH keypair.
    ///
    /// Uses X3DH::respond to derive the same shared secret the sender used,
    /// then decrypts the payload to recover the sender's information.
    ///
    /// # Arguments
    /// * `our_keys` - Our X3DH keypair
    ///
    /// # Returns
    /// A tuple of (DecryptedExchangePayload, shared_secret) containing the sender's
    /// identity key, exchange key, and display name.
    pub fn decrypt(
        &self,
        our_keys: &X3DHKeyPair,
    ) -> Result<(DecryptedExchangePayload, SymmetricKey), ExchangeError> {
        // Derive the shared secret using X3DH::respond
        // Note: X3DH::respond ignores the identity key parameter in this implementation
        let shared_secret = X3DH::respond(our_keys, &[0u8; 32], &self.ephemeral_public_key)?;

        // Decrypt the ciphertext
        let payload_bytes =
            decrypt(&shared_secret, &self.ciphertext).map_err(|_| ExchangeError::CryptoError)?;

        // Deserialize the payload
        let payload: ExchangePayload = serde_json::from_slice(&payload_bytes)
            .map_err(|_| ExchangeError::SerializationFailed)?;

        Ok((
            DecryptedExchangePayload {
                identity_key: payload.identity_key,
                exchange_key: payload.exchange_key,
                display_name: payload.display_name,
            },
            shared_secret,
        ))
    }

    /// Decrypts an exchange message (legacy API for backwards compatibility with tests).
    #[doc(hidden)]
    pub fn decrypt_legacy(
        &self,
        our_keys: &X3DHKeyPair,
        _their_identity_public: &[u8; 32],
    ) -> Result<([u8; 32], String), ExchangeError> {
        let (payload, _) = self.decrypt(our_keys)?;
        Ok((payload.identity_key, payload.display_name))
    }

    /// Serializes the message to bytes for wire transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Deserializes a message from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ExchangeError> {
        serde_json::from_slice(bytes).map_err(|_| ExchangeError::SerializationFailed)
    }
}
