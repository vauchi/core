// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Persistable application password credentials.

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
