//! Ed25519 Digital Signatures
//!
//! Provides signing keypair generation and signature operations using the
//! audited `ring` cryptographic library.

use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair as RingKeyPair};
use zeroize::Zeroize;

/// Ed25519 signing keypair for identity and message signing.
///
/// Private key material is zeroed on drop for security.
pub struct SigningKeyPair {
    keypair: Ed25519KeyPair,
    seed: [u8; 32],
}

impl Drop for SigningKeyPair {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

impl SigningKeyPair {
    /// Generates a new random Ed25519 keypair.
    ///
    /// Uses system random number generator for key material.
    pub fn generate() -> Self {
        let rng = SystemRandom::new();
        let seed = ring::rand::generate::<[u8; 32]>(&rng)
            .expect("System RNG should not fail")
            .expose();

        Self::from_seed(&seed)
    }

    /// Creates a keypair from a 32-byte seed.
    ///
    /// The same seed will always produce the same keypair,
    /// enabling deterministic key recovery from backups.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let keypair = Ed25519KeyPair::from_seed_unchecked(seed)
            .expect("Seed should be valid for Ed25519");

        SigningKeyPair {
            keypair,
            seed: *seed,
        }
    }

    /// Returns the public key portion of this keypair.
    pub fn public_key(&self) -> PublicKey {
        PublicKey {
            bytes: self.keypair.public_key().as_ref().try_into()
                .expect("Ed25519 public key is always 32 bytes"),
        }
    }

    /// Signs a message and returns the signature.
    pub fn sign(&self, message: &[u8]) -> Signature {
        let sig = self.keypair.sign(message);
        Signature {
            bytes: sig.as_ref().try_into()
                .expect("Ed25519 signature is always 64 bytes"),
        }
    }
}

/// Ed25519 public key for verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey {
    bytes: [u8; 32],
}

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
        use ring::signature::{UnparsedPublicKey, ED25519};

        let public_key = UnparsedPublicKey::new(&ED25519, &self.bytes);
        public_key.verify(message, &signature.bytes).is_ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let kp = SigningKeyPair::generate();
        assert_eq!(kp.public_key().as_bytes().len(), 32);
    }

    #[test]
    fn test_sign_verify() {
        let kp = SigningKeyPair::generate();
        let msg = b"test message";
        let sig = kp.sign(msg);
        assert!(kp.public_key().verify(msg, &sig));
    }
}
