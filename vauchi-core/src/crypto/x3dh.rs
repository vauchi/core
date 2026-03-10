// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! X25519 keypair for X3DH key agreement.
//!
//! This type is placed in the crypto module because it is a pure cryptographic
//! primitive (X25519 keypair) used by both exchange and identity modules.
//! The X3DH *protocol* logic remains in `exchange::x3dh`.

use rand::rngs::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};

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

    /// Performs Diffie-Hellman key agreement with a public key (from bytes).
    ///
    /// Returns the 32-byte shared secret.
    pub fn diffie_hellman(&self, their_public: &[u8; 32]) -> [u8; 32] {
        let their_public_key = PublicKey::from(*their_public);
        self.diffie_hellman_raw(&their_public_key)
    }

    /// Performs Diffie-Hellman key agreement with an x25519 PublicKey directly.
    ///
    /// Used by the X3DH protocol where the peer key is already parsed.
    pub(crate) fn diffie_hellman_raw(&self, their_public: &PublicKey) -> [u8; 32] {
        let shared = self.secret.diffie_hellman(their_public);
        *shared.as_bytes()
    }
}
