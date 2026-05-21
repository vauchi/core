// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Password-Based Key Derivation
//!
//! Provides Argon2id key derivation for password-based encryption (backups).
//!
//! Argon2id parameters: m=64MB, t=3, p=4 (OWASP recommended).
//!
//! When the `test-kdf` feature is enabled, memory cost is reduced to 8 MB and
//! time cost to 1 iteration for faster test execution. This feature MUST NOT
//! be enabled in production builds.

// Compile-time guard: test-kdf MUST NOT be enabled in release builds.
// It reduces Argon2id parameters to insecure levels for fast test execution.
#[cfg(all(feature = "test-kdf", not(debug_assertions)))]
compile_error!(
    "The `test-kdf` feature weakens Argon2id parameters and MUST NOT be enabled in release builds. \
     Use `--features test-kdf` only with debug/test profiles."
);

use zeroize::Zeroize;

use super::SymmetricKey;

/// Argon2id memory cost in KiB.
#[cfg(not(feature = "test-kdf"))]
pub(crate) const ARGON2_M_COST: u32 = 65536; // 64 MB (OWASP recommended)
#[cfg(feature = "test-kdf")]
pub(crate) const ARGON2_M_COST: u32 = 8 * 1024; // 8 MB (reduced for fast tests)

/// Argon2id time cost (iterations).
#[cfg(not(feature = "test-kdf"))]
pub(crate) const ARGON2_T_COST: u32 = 3;
#[cfg(feature = "test-kdf")]
pub(crate) const ARGON2_T_COST: u32 = 1; // 1 iteration (reduced for fast tests)

/// Argon2id parallelism.
pub(crate) const ARGON2_P_COST: u32 = 4;

/// Derives a 32-byte symmetric key from a password using Argon2id.
///
/// Parameters: m=64MB, t=3, p=4 per OWASP recommendations.
#[tracing::instrument(level = "debug", skip_all, name = "crypto.argon2id_derive")]
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

/// Password KDF error types.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PasswordKdfError {
    #[error("Key derivation failed: {0}")]
    DerivationFailed(String),
}

// Compile-time floor guard against silent weakening of Argon2id
// parameters. In any build that does not opt into `test-kdf`, the
// production constants must meet (or exceed) the OWASP floor:
//   - memory cost ≥ 64 MiB (65536 KiB)
//   - time cost   ≥ 3 iterations
//   - parallelism ≥ 4
// A weakening edit causes the crate to fail to compile in every
// configuration the released binaries ship with, including every CI
// job that does not pass `--features test-kdf`. Matches the existing
// `compile_error!` pattern at the top of this module rather than a
// runtime test, because runtime tests in this crate run *with*
// `test-kdf` enabled and so could never observe a weakening of the
// production values.
//
// Record: _private/docs/problems/2026-05-21-argon2-no-bench-no-floor-test/
#[cfg(not(feature = "test-kdf"))]
const _: () = {
    assert!(
        ARGON2_M_COST >= 65536,
        "ARGON2_M_COST below OWASP floor (64 MiB / 65536 KiB)"
    );
    assert!(
        ARGON2_T_COST >= 3,
        "ARGON2_T_COST below OWASP floor (3 iterations)"
    );
    assert!(
        ARGON2_P_COST >= 4,
        "ARGON2_P_COST below OWASP floor (4 lanes)"
    );
};
