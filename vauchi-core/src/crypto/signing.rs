// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ed25519 Digital Signatures
//!
//! Provides signing keypair generation and signature operations using the
//! `ed25519-dalek` cryptographic library.

use ed25519_dalek::{Signer, Verifier};
use subtle::ConstantTimeEq;

/// Ed25519 signing keypair for identity and message signing.
///
/// Wraps `ed25519_dalek::SigningKey`, which implements `ZeroizeOnDrop`
/// — both the seed and expanded key material are zeroed on drop.
pub struct SigningKeyPair {
    inner: ed25519_dalek::SigningKey,
}

impl SigningKeyPair {
    /// Generates a new random Ed25519 keypair.
    ///
    /// Uses system random number generator for key material.
    pub fn generate() -> Self {
        let seed = zeroize::Zeroizing::new(super::random_bytes::<32>());
        Self::from_seed(&seed)
    }

    /// Creates a keypair from a 32-byte seed.
    ///
    /// The same seed will always produce the same keypair,
    /// enabling deterministic key recovery from backups.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let inner = ed25519_dalek::SigningKey::from_bytes(seed);
        SigningKeyPair { inner }
    }

    /// Returns the public key portion of this keypair.
    pub fn public_key(&self) -> PublicKey {
        let vk = self.inner.verifying_key();
        PublicKey {
            bytes: vk.to_bytes(),
        }
    }

    /// Signs a message and returns the signature.
    pub fn sign(&self, message: &[u8]) -> Signature {
        let sig = self.inner.sign(message);
        Signature {
            bytes: sig.to_bytes(),
        }
    }
}

/// Ed25519 public key for verification.
///
/// Uses constant-time comparison to prevent timing side-channels when
/// comparing public keys (e.g., self-exchange detection, contact lookup).
#[derive(Clone, Debug)]
pub struct PublicKey {
    bytes: [u8; 32],
}

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.bytes.ct_eq(&other.bytes))
    }
}

impl Eq for PublicKey {}

impl PublicKey {
    /// Creates a public key from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        PublicKey { bytes }
    }

    /// Returns the raw bytes of the public key.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Returns a human-readable hex fingerprint of the public key.
    ///
    /// The fingerprint is the full hex encoding of the public key,
    /// suitable for display and manual verification.
    pub fn fingerprint(&self) -> String {
        hex::encode(self.bytes)
    }

    /// Verifies a signature against a message using this public key.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(&self.bytes) else {
            return false;
        };
        let sig = ed25519_dalek::Signature::from_bytes(&signature.bytes);
        vk.verify(message, &sig).is_ok()
    }
}

/// Ed25519 signature (64 bytes).
#[derive(Clone, Debug)]
pub struct Signature {
    bytes: [u8; 64],
}

impl Signature {
    /// Creates a signature from raw bytes.
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Signature { bytes }
    }

    /// Returns the raw bytes of the signature.
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.bytes
    }
}
