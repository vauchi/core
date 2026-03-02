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

use ring::hmac;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

/// KDF error types.
#[derive(Error, Debug)]
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
        let default_salt = [0u8; 32];
        let salt_bytes = salt.unwrap_or(&default_salt);
        let key = hmac::Key::new(hmac::HMAC_SHA256, salt_bytes);
        let tag = hmac::sign(&key, ikm);
        let mut prk = [0u8; 32];
        prk.copy_from_slice(tag.as_ref());
        prk
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
        const HASH_LEN: usize = 32;
        const MAX_OUTPUT: usize = 255 * HASH_LEN;

        if length > MAX_OUTPUT {
            return Err(KDFError::OutputTooLong);
        }

        if length == 0 {
            return Ok(Vec::new());
        }

        let key = hmac::Key::new(hmac::HMAC_SHA256, prk);
        let n = length.div_ceil(HASH_LEN);

        let mut okm = Vec::with_capacity(n * HASH_LEN);
        let mut t_prev: Vec<u8> = Vec::new();

        for i in 1..=n {
            // T(i) = HMAC(PRK, T(i-1) || info || i)
            let mut input = Vec::with_capacity(t_prev.len() + info.len() + 1);
            input.extend_from_slice(&t_prev);
            input.extend_from_slice(info);
            input.push(i as u8);

            let tag = hmac::sign(&key, &input);

            // Zeroize intermediate buffers before overwriting
            input.zeroize();
            t_prev.zeroize();

            t_prev = tag.as_ref().to_vec();
            okm.extend_from_slice(&t_prev);
        }

        // Zeroize the last T(N) block
        t_prev.zeroize();

        // Zeroize tail bytes beyond requested length before truncation.
        // These are derived key material in the Vec's spare capacity.
        okm[length..].zeroize();
        okm.truncate(length);
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
        let mut prk = Self::extract(salt, ikm);
        let result = Self::expand(&prk, info, length);
        prk.zeroize();
        result
    }

    /// Derives a fixed-size 32-byte key, wrapped in `Zeroizing` for automatic
    /// cleanup when the caller's variable goes out of scope.
    ///
    /// Convenience method for the common case of deriving a single symmetric key.
    /// The intermediate PRK and OKM buffer are zeroized after extraction.
    pub fn derive_key(salt: Option<&[u8]>, ikm: &[u8], info: &[u8]) -> Zeroizing<[u8; 32]> {
        let mut prk = Self::extract(salt, ikm);
        // expand for exactly 32 bytes can't fail
        let mut okm = Self::expand(&prk, info, 32).expect("32 bytes is valid length");
        prk.zeroize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&okm);
        okm.zeroize();
        Zeroizing::new(key)
    }

    /// Derives two 32-byte keys from the same input.
    ///
    /// Used in Double Ratchet for deriving (root_key, chain_key) pairs.
    /// The intermediate PRK and OKM buffer are zeroized after extraction.
    pub fn derive_key_pair(salt: Option<&[u8]>, ikm: &[u8], info: &[u8]) -> ([u8; 32], [u8; 32]) {
        let mut prk = Self::extract(salt, ikm);
        let mut okm = Self::expand(&prk, info, 64).expect("64 bytes is valid length");
        prk.zeroize();
        let mut key1 = [0u8; 32];
        let mut key2 = [0u8; 32];
        key1.copy_from_slice(&okm[..32]);
        key2.copy_from_slice(&okm[32..]);
        okm.zeroize();
        (key1, key2)
    }
}
