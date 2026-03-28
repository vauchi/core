// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! HKDF Key Derivation Function
//!
//! Implements HMAC-based Extract-and-Expand Key Derivation Function (HKDF)
//! as specified in RFC 5869. Used for deriving cryptographic keys in the
//! Double Ratchet protocol.
//!
//! All intermediate key material (PRK, T(i) blocks) is zeroized after use
//! to prevent key extraction from memory dumps (#234).

use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;
use zeroize::Zeroizing;

/// KDF error types.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum KDFError {
    #[error("Output length exceeds maximum (255 * hash_len)")]
    OutputTooLong,
    #[error("Invalid PRK length")]
    InvalidPRKLength,
}

/// HKDF-SHA256 key derivation.
///
/// Implements the Extract-and-Expand paradigm from RFC 5869.
pub struct HKDF;

impl HKDF {
    /// HKDF Extract: Creates a pseudorandom key (PRK) from input key material.
    ///
    /// PRK = HMAC-SHA256(salt, IKM)
    ///
    /// If salt is None, uses a string of HashLen zeros.
    pub fn extract(salt: Option<&[u8]>, ikm: &[u8]) -> [u8; 32] {
        let (prk, _) = Hkdf::<Sha256>::extract(salt, ikm);
        let mut out = [0u8; 32];
        out.copy_from_slice(&prk);
        out
    }

    /// HKDF Expand: Expands a PRK into output keying material.
    ///
    /// OKM = T(1) || T(2) || ... || T(N)
    /// where T(i) = HMAC-SHA256(PRK, T(i-1) || info || i)
    ///
    /// Maximum output length is 255 * 32 = 8160 bytes.
    ///
    /// Intermediate buffers (T(i-1), input concatenation) are zeroized after use.
    pub fn expand(prk: &[u8; 32], info: &[u8], length: usize) -> Result<Vec<u8>, KDFError> {
        if length == 0 {
            return Ok(Vec::new());
        }

        let hk = Hkdf::<Sha256>::from_prk(prk).map_err(|_| KDFError::InvalidPRKLength)?;
        let mut okm = vec![0u8; length];
        hk.expand(info, &mut okm)
            .map_err(|_| KDFError::OutputTooLong)?;
        Ok(okm)
    }

    /// Full HKDF: Extract-then-Expand in one step.
    ///
    /// This is the most common usage pattern.
    /// The intermediate PRK is zeroized after expansion.
    pub fn derive(
        salt: Option<&[u8]>,
        ikm: &[u8],
        info: &[u8],
        length: usize,
    ) -> Result<Vec<u8>, KDFError> {
        let hk = Hkdf::<Sha256>::new(salt, ikm);
        let mut okm = vec![0u8; length];
        hk.expand(info, &mut okm)
            .map_err(|_| KDFError::OutputTooLong)?;
        Ok(okm)
    }

    /// Derives a fixed-size 32-byte key, wrapped in `Zeroizing` for automatic
    /// cleanup when the caller's variable goes out of scope.
    ///
    /// Convenience method for the common case of deriving a single symmetric key.
    /// The intermediate PRK and OKM buffer are zeroized after extraction.
    pub fn derive_key(salt: Option<&[u8]>, ikm: &[u8], info: &[u8]) -> Zeroizing<[u8; 32]> {
        let hk = Hkdf::<Sha256>::new(salt, ikm);
        let mut okm = [0u8; 32];
        hk.expand(info, &mut okm).expect("32 bytes is valid length");
        Zeroizing::new(okm)
    }

    /// Derives two 32-byte keys from the same input.
    ///
    /// Used in Double Ratchet for deriving (root_key, chain_key) pairs.
    /// The intermediate PRK and OKM buffer are zeroized after extraction.
    pub fn derive_key_pair(
        salt: Option<&[u8]>,
        ikm: &[u8],
        info: &[u8],
    ) -> (Zeroizing<[u8; 32]>, Zeroizing<[u8; 32]>) {
        let hk = Hkdf::<Sha256>::new(salt, ikm);
        let mut okm = Zeroizing::new([0u8; 64]);
        hk.expand(info, okm.as_mut())
            .expect("64 bytes is valid length");
        let mut key1 = [0u8; 32];
        let mut key2 = [0u8; 32];
        key1.copy_from_slice(&okm[..32]);
        key2.copy_from_slice(&okm[32..]);
        (Zeroizing::new(key1), Zeroizing::new(key2))
    }
}
