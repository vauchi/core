// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! App-Level Password / Duress PIN Authentication
//!
//! Provides Argon2id-based password hashing and verification for the
//! app-level password and duress PIN features.
//!
//! Uses constant-time comparison via the `subtle` crate to prevent
//! timing side-channel attacks. Both the normal hash and duress hash
//! are always checked (even if one matches) to ensure uniform timing.

use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use subtle::ConstantTimeEq;

use super::error::{VauchiError, VauchiResult};

/// Argon2id memory cost in KiB — matches password_kdf module.
#[cfg(not(feature = "test-kdf"))]
const ARGON2_M_COST: u32 = 65536; // 64 MB (OWASP recommended)
#[cfg(feature = "test-kdf")]
const ARGON2_M_COST: u32 = 8 * 1024; // 8 MB (reduced for fast tests)

/// Argon2id time cost (iterations).
#[cfg(not(feature = "test-kdf"))]
const ARGON2_T_COST: u32 = 3;
#[cfg(feature = "test-kdf")]
const ARGON2_T_COST: u32 = 1; // 1 iteration (reduced for fast tests)

/// Argon2id parallelism.
const ARGON2_P_COST: u32 = 4;

/// Result of password verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthResult {
    /// The normal (real) password was entered.
    Normal,
    /// The duress PIN was entered — activate duress mode.
    Duress,
    /// Neither password matched.
    Invalid,
}

/// App password configuration with hashed credentials.
///
/// Stores Argon2id hashes and salts for the normal app password and
/// optional duress PIN. Verification uses constant-time comparison
/// and always checks both hashes to prevent timing attacks.
#[derive(Clone)]
pub struct AppPasswordConfig {
    password_hash: [u8; 32],
    password_salt: [u8; 16],
    duress_hash: Option<[u8; 32]>,
    duress_salt: Option<[u8; 16]>,
    duress_enabled: bool,
}

impl AppPasswordConfig {
    /// Creates a new password configuration by hashing the given password.
    ///
    /// Generates a random 16-byte salt and derives a 32-byte hash
    /// using Argon2id with OWASP-recommended parameters.
    pub fn create(password: &str) -> VauchiResult<Self> {
        let salt = generate_salt()?;
        let hash = hash_password(password, &salt)?;

        Ok(AppPasswordConfig {
            password_hash: hash,
            password_salt: salt,
            duress_hash: None,
            duress_salt: None,
            duress_enabled: false,
        })
    }

    /// Sets up a duress PIN on this configuration.
    ///
    /// The duress password must differ from the normal password.
    /// Generates a separate salt and Argon2id hash.
    pub fn setup_duress(&mut self, duress_password: &str) -> VauchiResult<()> {
        // Verify the duress password is different from the normal password
        let candidate_hash = hash_password(duress_password, &self.password_salt)?;
        if bool::from(candidate_hash.ct_eq(&self.password_hash)) {
            return Err(VauchiError::InvalidState(
                "duress password must differ from normal password".into(),
            ));
        }

        let salt = generate_salt()?;
        let hash = hash_password(duress_password, &salt)?;

        self.duress_hash = Some(hash);
        self.duress_salt = Some(salt);
        self.duress_enabled = true;

        Ok(())
    }

    /// Verifies a password attempt against the stored hashes.
    ///
    /// Both the normal and duress hashes are always checked to prevent
    /// timing side-channel leakage. Returns `AuthResult::Normal` if the
    /// normal password matches, `AuthResult::Duress` if the duress PIN
    /// matches, or `AuthResult::Invalid` if neither matches.
    pub fn verify(&self, password: &str) -> AuthResult {
        // Always hash against the normal salt
        let normal_hash = match hash_password(password, &self.password_salt) {
            Ok(h) => h,
            Err(_) => return AuthResult::Invalid,
        };
        let normal_match = bool::from(normal_hash.ct_eq(&self.password_hash));

        // Always hash against the duress salt (if present) — even if normal matched
        let duress_match = if let (Some(duress_salt), Some(duress_hash)) =
            (&self.duress_salt, &self.duress_hash)
        {
            match hash_password(password, duress_salt) {
                Ok(h) => bool::from(h.ct_eq(duress_hash)),
                Err(_) => false,
            }
        } else {
            false
        };

        // Determine result — normal takes priority over duress
        if normal_match {
            AuthResult::Normal
        } else if duress_match {
            AuthResult::Duress
        } else {
            AuthResult::Invalid
        }
    }

    /// Constructs an `AppPasswordConfig` from raw stored values.
    ///
    /// Used by the storage layer when loading persisted password config.
    pub fn from_stored(
        password_hash: [u8; 32],
        password_salt: [u8; 16],
        duress_hash: Option<[u8; 32]>,
        duress_salt: Option<[u8; 16]>,
        duress_enabled: bool,
    ) -> Self {
        AppPasswordConfig {
            password_hash,
            password_salt,
            duress_hash,
            duress_salt,
            duress_enabled,
        }
    }

    /// Returns the password hash.
    pub fn password_hash(&self) -> &[u8; 32] {
        &self.password_hash
    }

    /// Returns the password salt.
    pub fn password_salt(&self) -> &[u8; 16] {
        &self.password_salt
    }

    /// Returns the duress hash, if configured.
    pub fn duress_hash(&self) -> Option<&[u8; 32]> {
        self.duress_hash.as_ref()
    }

    /// Returns the duress salt, if configured.
    pub fn duress_salt(&self) -> Option<&[u8; 16]> {
        self.duress_salt.as_ref()
    }

    /// Returns whether duress mode is enabled.
    pub fn duress_enabled(&self) -> bool {
        self.duress_enabled
    }
}

/// Generates a cryptographically random 16-byte salt.
fn generate_salt() -> VauchiResult<[u8; 16]> {
    let rng = SystemRandom::new();
    let mut salt = [0u8; 16];
    rng.fill(&mut salt)
        .map_err(|_| VauchiError::Crypto("failed to generate salt".into()))?;
    Ok(salt)
}

/// Hashes a password with Argon2id using the given salt.
///
/// Parameters: m=64MB, t=3, p=4 (OWASP recommended).
fn hash_password(password: &str, salt: &[u8]) -> VauchiResult<[u8; 32]> {
    let params = argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
        .map_err(|e| VauchiError::Crypto(format!("argon2 params: {}", e)))?;

    let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut hash = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut hash)
        .map_err(|e| VauchiError::Crypto(format!("argon2 hash: {}", e)))?;

    Ok(hash)
}
