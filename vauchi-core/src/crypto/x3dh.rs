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
use zeroize::Zeroizing;

/// Error returned when a Diffie-Hellman computation produces a non-contributory output
/// (e.g., the all-zero shared secret from a small-subgroup public key).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Non-contributory DH output (possible small-subgroup attack)")]
pub struct DhError;

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
    /// Wrapped in `Zeroizing` so callers don't leak key material on the stack.
    pub fn secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.secret.to_bytes())
    }

    /// Performs Diffie-Hellman key agreement with a public key (from bytes).
    ///
    /// Returns the 32-byte shared secret wrapped in `Zeroizing` for automatic
    /// cleanup, or `DhError` if the output is non-contributory (e.g., peer
    /// sent a small-subgroup point).
    pub fn diffie_hellman(&self, their_public: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>, DhError> {
        let their_public_key = PublicKey::from(*their_public);
        self.diffie_hellman_raw(&their_public_key)
    }

    /// Performs Diffie-Hellman key agreement with an x25519 PublicKey directly.
    ///
    /// Used by the X3DH protocol where the peer key is already parsed.
    /// Returns `DhError` if the shared secret is non-contributory.
    pub(crate) fn diffie_hellman_raw(
        &self,
        their_public: &PublicKey,
    ) -> Result<Zeroizing<[u8; 32]>, DhError> {
        let shared = self.secret.diffie_hellman(their_public);
        if !shared.was_contributory() {
            return Err(DhError);
        }
        Ok(Zeroizing::new(*shared.as_bytes()))
    }
}

// No manual Drop needed: x25519-dalek's StaticSecret implements ZeroizeOnDrop.
// PublicKey is not secret material.
