// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Full X3DH Key Agreement
//!
//! This module implements X3DH with identity binding for contact card
//! exchange. Two DH operations per side provide both forward secrecy
//! (via the ephemeral key) and identity binding (via static keys).
//!
//! ## DH Operations
//!
//! | Operation | Initiator side             | Responder side            | Purpose          |
//! |-----------|----------------------------|---------------------------|------------------|
//! | DH1       | our_static × their_static  | our_static × their_static | Identity binding |
//! | DH2       | ephemeral × their_static   | our_static × their_ephemeral | Forward secrecy |
//!
//! The shared secret is derived as `HKDF(DH1 ‖ DH2, info="vauchi-x3dh-key-v2")`.
//!
//! ## Security Properties
//!
//! - **Forward secrecy**: Compromising long-term keys does not reveal past sessions
//!   (ephemeral key is destroyed after use).
//! - **Identity binding**: The shared secret is cryptographically tied to both
//!   parties' long-term X25519 keys, preventing key-compromise impersonation.

use rand::rngs::OsRng;
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroize;

use super::ExchangeError;
use crate::crypto::SymmetricKey;
pub use crate::crypto::X3DHKeyPair;
use crate::crypto::kdf::HKDF;

/// Domain separation info for X3DH key derivation via HKDF.
///
/// Bumped from v1 to v2 when identity binding (DH1) was added.
/// The IKM is now 64 bytes (DH1 ‖ DH2) instead of 32 bytes (DH2 only).
const X3DH_KEY_INFO: &[u8] = b"vauchi-x3dh-key-v2";

/// X3DH protocol implementation.
///
/// Provides methods for initiating and responding to key agreement.
pub struct X3DH;

impl X3DH {
    /// Initiates key agreement as the initiator (scanner).
    ///
    /// Performs two DH operations:
    /// - DH1: our_static × their_static (identity binding)
    /// - DH2: ephemeral × their_static (forward secrecy)
    ///
    /// Returns: (shared_secret, ephemeral_public_key_to_send)
    pub fn initiate(
        our_keys: &X3DHKeyPair,
        their_public: &[u8; 32],
    ) -> Result<(SymmetricKey, [u8; 32]), ExchangeError> {
        let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
        let ephemeral_public = PublicKey::from(&ephemeral_secret);

        let their_public_key = PublicKey::from(*their_public);

        // DH1: our_static × their_static (identity binding)
        let dh1 = our_keys.diffie_hellman(their_public)?;

        // DH2: ephemeral × their_static (forward secrecy)
        let dh2_shared = ephemeral_secret.diffie_hellman(&their_public_key);
        if !dh2_shared.was_contributory() {
            return Err(ExchangeError::InvalidDhOutput(crate::crypto::DhError));
        }
        let dh2 = *dh2_shared.as_bytes();

        // Concatenate DH1 ‖ DH2 and derive via HKDF
        let mut ikm = [0u8; 64];
        ikm[..32].copy_from_slice(&*dh1);
        ikm[32..].copy_from_slice(&dh2);
        let derived = HKDF::derive_key(None, &ikm, X3DH_KEY_INFO);
        ikm.zeroize();
        let key = SymmetricKey::from_bytes(*derived);

        Ok((key, *ephemeral_public.as_bytes()))
    }

    /// Responds to key agreement as the responder (QR displayer).
    ///
    /// Performs two DH operations (mirrors initiator):
    /// - DH1: our_static × their_static (identity binding)
    /// - DH2: our_static × their_ephemeral (forward secrecy)
    pub fn respond(
        our_keys: &X3DHKeyPair,
        their_identity_public: &[u8; 32],
        their_ephemeral_public: &[u8; 32],
    ) -> Result<SymmetricKey, ExchangeError> {
        let their_ephemeral = PublicKey::from(*their_ephemeral_public);

        // DH1: our_static × their_static (identity binding — mirrors initiator)
        let dh1 = our_keys.diffie_hellman(their_identity_public)?;

        // DH2: our_static × their_ephemeral (forward secrecy)
        let dh2 = our_keys.diffie_hellman_raw(&their_ephemeral)?;

        // Concatenate DH1 ‖ DH2 and derive via HKDF
        let mut ikm = [0u8; 64];
        ikm[..32].copy_from_slice(&*dh1);
        ikm[32..].copy_from_slice(&*dh2);
        let derived = HKDF::derive_key(None, &ikm, X3DH_KEY_INFO);
        ikm.zeroize();
        let key = SymmetricKey::from_bytes(*derived);

        Ok(key)
    }
}
