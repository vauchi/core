// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport-agnostic exchange protocol.
//!
//! Implements X25519 key agreement with XChaCha20-Poly1305 encryption,
//! independent of the underlying transport (BLE, WiFi Aware, QR, etc.).
//!
//! ## Offer format (90 bytes)
//!
//! | Offset | Size | Field             |
//! |--------|------|-------------------|
//! | 0      | 32   | Identity pub key  |
//! | 32     | 32   | Ephemeral pub key |
//! | 64     | 16   | Nonce             |
//! | 80     | 8    | Timestamp (BE)    |
//! | 88     | 2    | Capabilities (BE) |
//!
//! ## Key derivation
//!
//! ```text
//! dh_secret = X25519(our_ephemeral_secret, their_ephemeral_pub)
//! salt      = sorted(our_nonce, their_nonce)
//! info      = b"vauchi-transport-v3"
//! shared    = HKDF-SHA256(salt, dh_secret, info)
//! ```

use crate::crypto::kdf::HKDF;
use crate::exchange::error::ExchangeError;
use crate::exchange::transport::caps::TransportCaps;
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit};
use rand::RngCore;
use rand::rngs::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, Zeroizing};

/// Size of the offer payload in bytes.
const OFFER_SIZE: usize = 90;

/// Size of the nonce in the offer.
const NONCE_SIZE: usize = 16;

/// Size of the XChaCha20-Poly1305 nonce prepended to ciphertext.
const XCHACHA_NONCE_SIZE: usize = 24;

/// HKDF info string for transport key derivation.
const HKDF_INFO: &[u8] = b"vauchi-transport-v3";

/// Shared key derived from X25519 DH + HKDF. Zeroized on drop.
#[derive(Zeroize)]
#[zeroize(drop)]
pub struct SharedKey {
    bytes: [u8; 32],
}

impl SharedKey {
    /// Returns a reference to the raw key bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

/// Transport-agnostic exchange protocol using X25519 + XChaCha20-Poly1305.
pub struct ExchangeProtocol {
    /// Long-term identity secret (used for identity pub in offer).
    identity_secret: StaticSecret,
    /// Ephemeral secret for this exchange session.
    ephemeral_secret: StaticSecret,
    /// Random nonce included in offer.
    nonce: [u8; NONCE_SIZE],
    /// Transport capabilities to advertise.
    caps: TransportCaps,
}

impl ExchangeProtocol {
    /// Creates a new protocol instance with fresh random keys.
    pub fn new_random() -> Self {
        let identity_secret = StaticSecret::random_from_rng(OsRng);
        let ephemeral_secret = StaticSecret::random_from_rng(OsRng);

        let mut nonce = [0u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce);

        Self {
            identity_secret,
            ephemeral_secret,
            nonce,
            caps: TransportCaps::empty(),
        }
    }

    /// Sets the transport capabilities for this protocol instance.
    /// Returns self for method chaining.
    pub fn with_capabilities(mut self, caps: TransportCaps) -> Self {
        self.caps = caps;
        self
    }

    /// Creates an offer payload (90 bytes).
    ///
    /// Layout: identity_pub(32) + ephemeral_pub(32) + nonce(16) + timestamp(8) + caps(2)
    pub fn create_offer(&self) -> Result<Vec<u8>, ExchangeError> {
        let identity_pub = PublicKey::from(&self.identity_secret);
        let ephemeral_pub = PublicKey::from(&self.ephemeral_secret);

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ExchangeError::CryptoError)?
            .as_secs();

        let mut offer = Vec::with_capacity(OFFER_SIZE);
        offer.extend_from_slice(identity_pub.as_bytes());
        offer.extend_from_slice(ephemeral_pub.as_bytes());
        offer.extend_from_slice(&self.nonce);
        offer.extend_from_slice(&timestamp.to_be_bytes());
        offer.extend_from_slice(&self.caps.to_bytes());

        debug_assert_eq!(offer.len(), OFFER_SIZE);
        Ok(offer)
    }

    /// Processes a peer's offer and derives a shared key via X25519 DH + HKDF.
    ///
    /// The HKDF salt is the sorted concatenation of both nonces, ensuring
    /// both parties derive the same key regardless of who is "Alice" or "Bob".
    pub fn process_offer(&self, peer_offer: &[u8]) -> Result<SharedKey, ExchangeError> {
        if peer_offer.len() < OFFER_SIZE {
            return Err(ExchangeError::InvalidQRFormat);
        }

        // Parse peer ephemeral public key (bytes 32..64)
        let mut peer_ephemeral_bytes = [0u8; 32];
        peer_ephemeral_bytes.copy_from_slice(&peer_offer[32..64]);
        let peer_ephemeral = PublicKey::from(peer_ephemeral_bytes);

        // Parse peer nonce (bytes 64..80)
        let mut peer_nonce = [0u8; NONCE_SIZE];
        peer_nonce.copy_from_slice(&peer_offer[64..80]);

        // X25519 Diffie-Hellman
        let dh_secret = self.ephemeral_secret.diffie_hellman(&peer_ephemeral);
        if !dh_secret.was_contributory() {
            return Err(ExchangeError::InvalidDhOutput(crate::crypto::DhError));
        }

        // Build sorted salt from both nonces
        let mut salt = [0u8; NONCE_SIZE * 2];
        if self.nonce <= peer_nonce {
            salt[..NONCE_SIZE].copy_from_slice(&self.nonce);
            salt[NONCE_SIZE..].copy_from_slice(&peer_nonce);
        } else {
            salt[..NONCE_SIZE].copy_from_slice(&peer_nonce);
            salt[NONCE_SIZE..].copy_from_slice(&self.nonce);
        }

        // HKDF: extract-then-expand
        let derived = HKDF::derive_key(Some(&salt), dh_secret.as_bytes(), HKDF_INFO);

        // Zeroize intermediates
        let mut dh_bytes = *dh_secret.as_bytes();
        dh_bytes.zeroize();

        Ok(SharedKey {
            bytes: *Zeroizing::new(*derived),
        })
    }

    /// Encrypts card data with XChaCha20-Poly1305 using the shared key.
    ///
    /// Output format: nonce(24) + ciphertext + tag(16)
    pub fn encrypt_card(data: &[u8], shared: &SharedKey) -> Result<Vec<u8>, ExchangeError> {
        let cipher = XChaCha20Poly1305::new(shared.as_bytes().into());

        let mut nonce_bytes = [0u8; XCHACHA_NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, data)
            .map_err(|_| ExchangeError::CryptoError)?;

        let mut output = Vec::with_capacity(XCHACHA_NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        Ok(output)
    }

    /// Decrypts card data with XChaCha20-Poly1305 using the shared key.
    ///
    /// Expects input format: nonce(24) + ciphertext + tag(16)
    pub fn decrypt_card(encrypted: &[u8], shared: &SharedKey) -> Result<Vec<u8>, ExchangeError> {
        if encrypted.len() < XCHACHA_NONCE_SIZE + 16 {
            return Err(ExchangeError::CryptoError);
        }

        let nonce = chacha20poly1305::XNonce::from_slice(&encrypted[..XCHACHA_NONCE_SIZE]);
        let cipher = XChaCha20Poly1305::new(shared.as_bytes().into());

        cipher
            .decrypt(nonce, &encrypted[XCHACHA_NONCE_SIZE..])
            .map_err(|_| ExchangeError::CryptoError)
    }
}

impl Drop for ExchangeProtocol {
    fn drop(&mut self) {
        self.nonce.zeroize();
    }
}
