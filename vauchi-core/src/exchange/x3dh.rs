//! X3DH Key Agreement Protocol
//!
//! Implements a simplified X3DH-style key agreement for contact exchange.
//! Uses X25519 for Diffie-Hellman key agreement.

use rand::rngs::OsRng;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

use super::ExchangeError;
use crate::crypto::SymmetricKey;

/// X25519 keypair for X3DH key agreement.
///
/// Used for establishing shared secrets during contact exchange.
pub struct X3DHKeyPair {
    /// The static secret key
    secret: StaticSecret,
    /// The public key (cached for efficiency)
    public: PublicKey,
}

impl X3DHKeyPair {
    /// Generates a new random X25519 keypair.
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);

        X3DHKeyPair { secret, public }
    }

    /// Creates a keypair from a 32-byte seed.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);

        X3DHKeyPair { secret, public }
    }

    /// Returns the public key bytes.
    pub fn public_key(&self) -> &[u8; 32] {
        self.public.as_bytes()
    }

    /// Returns the secret key bytes (for backup/restore).
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }

    /// Performs Diffie-Hellman key agreement with a public key.
    ///
    /// Returns the 32-byte shared secret.
    pub fn diffie_hellman(&self, their_public: &[u8; 32]) -> [u8; 32] {
        let their_public_key = PublicKey::from(*their_public);
        let shared = self.secret.diffie_hellman(&their_public_key);
        *shared.as_bytes()
    }
}

/// X3DH protocol implementation.
///
/// Provides methods for initiating and responding to key agreement.
pub struct X3DH;

impl X3DH {
    /// Initiates key agreement as the initiator (scanner).
    ///
    /// The initiator generates an ephemeral keypair and performs DH with
    /// the responder's static public key.
    ///
    /// Returns: (shared_secret, ephemeral_public_key_to_send)
    pub fn initiate(
        _our_keys: &X3DHKeyPair,
        their_public: &[u8; 32],
    ) -> Result<(SymmetricKey, [u8; 32]), ExchangeError> {
        // Generate ephemeral key for this exchange
        let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
        let ephemeral_public = PublicKey::from(&ephemeral_secret);

        // Convert their public key
        let their_public_key = PublicKey::from(*their_public);

        // Perform DH: ephemeral_secret * their_public
        let shared_secret = ephemeral_secret.diffie_hellman(&their_public_key);

        // Derive symmetric key from shared secret
        let key = SymmetricKey::from_bytes(*shared_secret.as_bytes());

        Ok((key, *ephemeral_public.as_bytes()))
    }

    /// Responds to key agreement as the responder (QR displayer).
    ///
    /// The responder uses their static key to perform DH with the
    /// initiator's ephemeral public key.
    pub fn respond(
        our_keys: &X3DHKeyPair,
        _their_identity_public: &[u8; 32],
        their_ephemeral_public: &[u8; 32],
    ) -> Result<SymmetricKey, ExchangeError> {
        // Convert their ephemeral public key
        let their_ephemeral = PublicKey::from(*their_ephemeral_public);

        // Perform DH: our_secret * their_ephemeral
        let shared_secret = our_keys.secret.diffie_hellman(&their_ephemeral);

        // Derive symmetric key from shared secret
        let key = SymmetricKey::from_bytes(*shared_secret.as_bytes());

        Ok(key)
    }
}
