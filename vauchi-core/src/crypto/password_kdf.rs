// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Password-Based Key Derivation
//!
//! Provides Argon2id key derivation for new operations, with PBKDF2 fallback
//! for importing legacy data.
//!
//! Argon2id parameters: m=64MB, t=3, p=4 (OWASP recommended).

use ring::pbkdf2;
use std::num::NonZeroU32;
use zeroize::Zeroize;

use super::SymmetricKey;

/// Argon2id memory cost in KiB (64 MB).
const ARGON2_M_COST: u32 = 65536;
/// Argon2id time cost (iterations).
const ARGON2_T_COST: u32 = 3;
/// Argon2id parallelism.
const ARGON2_P_COST: u32 = 4;

/// PBKDF2 iterations for legacy key derivation.
const PBKDF2_ITERATIONS: u32 = 100_000;

/// Derives a 32-byte symmetric key from a password using Argon2id.
///
/// Parameters: m=64MB, t=3, p=4 per OWASP recommendations.
pub fn derive_key_argon2id(password: &[u8], salt: &[u8]) -> Result<SymmetricKey, PasswordKdfError> {
    let params = argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
        .map_err(|e| PasswordKdfError::DerivationFailed(e.to_string()))?;

    let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key_bytes = [0u8; 32];
    argon2
        .hash_password_into(password, salt, &mut key_bytes)
        .map_err(|e| PasswordKdfError::DerivationFailed(e.to_string()))?;

    let key = SymmetricKey::from_bytes(key_bytes);
    key_bytes.zeroize();
    Ok(key)
}

/// Derives a 32-byte symmetric key from a password using PBKDF2-HMAC-SHA256.
///
/// Used for decrypting legacy backups created before the Argon2id migration.
pub fn derive_key_pbkdf2(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
) -> Result<SymmetricKey, PasswordKdfError> {
    let mut key_bytes = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(iterations).ok_or(PasswordKdfError::DerivationFailed(
            "iterations must be non-zero".into(),
        ))?,
        salt,
        password,
        &mut key_bytes,
    );

    let key = SymmetricKey::from_bytes(key_bytes);
    key_bytes.zeroize();
    Ok(key)
}

/// Derives a 32-byte symmetric key using PBKDF2 with the default iteration count.
///
/// Convenience wrapper for `derive_key_pbkdf2` with `PBKDF2_ITERATIONS`.
pub fn derive_key_pbkdf2_default(
    password: &[u8],
    salt: &[u8],
) -> Result<SymmetricKey, PasswordKdfError> {
    derive_key_pbkdf2(password, salt, PBKDF2_ITERATIONS)
}

/// Password KDF error types.
#[derive(Debug, thiserror::Error)]
pub enum PasswordKdfError {
    #[error("Key derivation failed: {0}")]
    DerivationFailed(String),
}
