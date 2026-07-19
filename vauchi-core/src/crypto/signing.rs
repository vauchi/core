// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ed25519 Digital Signatures
//!
//! Provides signing keypair generation and signature operations using the
//! `ed25519-dalek` cryptographic library.

use ed25519_dalek::{Signer, Verifier};
use sha2::Digest;
use subtle::ConstantTimeEq;

/// Ed25519 signing keypair for identity and message signing.
///
/// Wraps `ed25519_dalek::SigningKey`, which implements `ZeroizeOnDrop`
/// — both the seed and expanded key material are zeroed on drop.
pub struct SigningKeyPair {
    inner: ed25519_dalek::SigningKey,
}

// Manual marker (not derived): dalek's SigningKey zeroizes itself on
// drop but does not expose `Zeroize`, so the derive cannot apply. The
// impl states the wrapper-level guarantee the VRS01 contract asserts.
// nosemgrep: vauchi-no-manual-zeroize-on-drop — dalek field owns its drop zeroization
impl zeroize::ZeroizeOnDrop for SigningKeyPair {}

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

    /// Derives the X25519 secret corresponding to this Ed25519 signing key.
    ///
    /// This is the libsodium `crypto_sign_ed25519_sk_to_curve25519`
    /// construction: the X25519 scalar is `clamp(SHA-512(seed)[0..32])` — the
    /// same clamped scalar Ed25519 signs with — so the matching public key is
    /// [`PublicKey::to_x25519`] (`VerifyingKey::to_montgomery`). It lets a
    /// guardian open sealed-box material addressed to its advertised *signing*
    /// key without carrying a separate encryption key. The clamped bytes are
    /// idempotent under x25519-dalek's own clamp, so the DH agrees with
    /// `to_montgomery`. Guardian-backup key reuse is sanctioned by the ADR-002
    /// amendment; the seal/open contract is proved by
    /// `guardian_identity_key_contract_tests`.
    pub fn to_x25519_secret(&self) -> x25519_dalek::StaticSecret {
        let seed = zeroize::Zeroizing::new(self.inner.to_bytes());
        let mut hash = zeroize::Zeroizing::new([0u8; 64]);
        hash.copy_from_slice(&sha2::Sha512::new().chain_update(&*seed).finalize());
        let mut scalar = zeroize::Zeroizing::new([0u8; 32]);
        scalar.copy_from_slice(&hash[..32]);
        scalar[0] &= 248;
        scalar[31] &= 127;
        scalar[31] |= 64;
        x25519_dalek::StaticSecret::from(*scalar)
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

    /// Maps this Ed25519 public key to its X25519 (Montgomery) form.
    ///
    /// Returns `None` if the bytes are not a valid Ed25519 point. The result
    /// is the sealed-box recipient key that pairs with
    /// [`SigningKeyPair::to_x25519_secret`] — used to address guardian entries
    /// by a contact's advertised signing key.
    pub fn to_x25519(&self) -> Option<x25519_dalek::PublicKey> {
        let vk = ed25519_dalek::VerifyingKey::from_bytes(&self.bytes).ok()?;
        Some(x25519_dalek::PublicKey::from(vk.to_montgomery().to_bytes()))
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

/// Verify an Ed25519 `signature` over `message` against a *peer's* public
/// key. Unlike [`SigningKeyPair::verify`] (which checks against the
/// keypair's own key), this verifies a signature made by someone else —
/// e.g. a link-mode bootstrap signed by a contact's identity key
/// (ADR-050). Returns `false` for a malformed public key or signature, so
/// callers fail closed.
pub fn verify_signature(public_key: &[u8; 32], message: &[u8], signature: &Signature) -> bool {
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let sig = ed25519_dalek::Signature::from_bytes(signature.as_bytes());
    vk.verify(message, &sig).is_ok()
}

// INLINE_TEST_REQUIRED: the guardian seal/open key contract is validated
// against crate-internal derivations (HKDF Vauchi_Exchange_Seed_v2 negative
// case via crypto::X3DHKeyPair) and the raw x25519_dalek primitive, neither of
// which is on the public integration-test surface. The end-to-end contract via
// the public API lives in tests/it/guardian_identity_key_contract_tests.rs.
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // The guardian seal/open contract, reduced to its primitive: the X25519
    // secret derived from an identity's signing key must have exactly the
    // public key that guardian entries are sealed to (VerifyingKey::to_
    // montgomery). Sealed-box then guarantees openability — proved end-to-end
    // in guardian_identity_key_contract_tests.
    #[test]
    fn x25519_secret_public_matches_montgomery_recipient() {
        let kp = SigningKeyPair::from_seed(&[42u8; 32]);
        let secret_pub = x25519_dalek::PublicKey::from(&kp.to_x25519_secret());
        let recipient = kp.public_key().to_x25519().expect("valid point");
        assert_eq!(secret_pub.as_bytes(), recipient.as_bytes());
    }

    // The pre-fix opener derived HKDF("Vauchi_Exchange_Seed_v2"); its public key
    // is unrelated to the seal target, which is why real identities could never
    // open their own guardian entries. Guards against regressing to it.
    #[test]
    fn exchange_seed_secret_is_not_the_guardian_recipient() {
        let seed = [7u8; 32];
        let kp = SigningKeyPair::from_seed(&seed);
        let recipient = kp.public_key().to_x25519().unwrap();
        let exchange_seed =
            crate::crypto::HKDF::derive_key(None, &seed, b"Vauchi_Exchange_Seed_v2");
        let x3dh = crate::crypto::X3DHKeyPair::from_bytes(*exchange_seed);
        assert_ne!(x3dh.public_key(), recipient.as_bytes());
    }

    proptest! {
        #[test]
        fn x25519_secret_public_matches_recipient_for_any_seed(seed: [u8; 32]) {
            let kp = SigningKeyPair::from_seed(&seed);
            let secret_pub = x25519_dalek::PublicKey::from(&kp.to_x25519_secret());
            let recipient = kp.public_key().to_x25519().expect("valid point");
            prop_assert_eq!(secret_pub.as_bytes(), recipient.as_bytes());
        }
    }
}
