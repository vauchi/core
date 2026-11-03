// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The owner's optional recovery passphrase over the guardian backup
//! (vauchi/private#796, D16).
//!
//! The guardian backup is already encrypted under a key the guardians'
//! shares rebuild. With a passphrase set it is sealed once more, under an
//! Argon2id key from the passphrase and a random salt, so guardians that
//! collude still get nothing without it. Wire form:
//! `MAGIC || salt(16) || XChaCha20-Poly1305(key, guardian backup)`.

use crate::crypto::{SymmetricKey, decrypt, derive_key_argon2id, encrypt};

/// Marks a guardian backup sealed under the owner's passphrase; a plain
/// guardian backup starts with its own version byte, never these four.
const MAGIC: &[u8; 4] = b"VRP1";
const SALT_BYTES: usize = 16;
/// How many hex characters spell the magic, enough to tell a sealed backup
/// without decoding the rest.
pub const MAGIC_HEX_LEN: usize = 2 * MAGIC.len();

/// Why a passphrase-sealed backup could not be sealed or opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PassphraseLayerError {
    /// The bytes are not a passphrase-sealed backup.
    #[error("Not a passphrase-sealed guardian backup")]
    NotSealed,
    /// The passphrase is wrong or the bytes were altered.
    #[error("The recovery passphrase does not open this backup")]
    WrongPassphrase,
    /// Deriving or sealing failed.
    #[error("Passphrase sealing failed: {0}")]
    Crypto(String),
}

/// Whether `bytes` is a guardian backup sealed under the owner's passphrase.
pub fn is_passphrase_sealed(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// The key a passphrase and its salt derive (Argon2id), kept so the daily
/// refresh can seal without asking for the passphrase again.
///
/// # Errors
/// [`PassphraseLayerError::Crypto`] when the derivation fails.
pub fn passphrase_key(
    passphrase: &str,
    salt: &[u8; SALT_BYTES],
) -> Result<SymmetricKey, PassphraseLayerError> {
    derive_key_argon2id(passphrase.as_bytes(), salt)
        .map_err(|e| PassphraseLayerError::Crypto(e.to_string()))
}

/// Seals a guardian backup under `key`, derived from the passphrase and
/// `salt`.
///
/// # Errors
/// [`PassphraseLayerError::Crypto`] when encryption fails.
pub fn seal(
    key: &SymmetricKey,
    salt: &[u8; SALT_BYTES],
    backup: &[u8],
) -> Result<Vec<u8>, PassphraseLayerError> {
    let sealed = encrypt(key, backup).map_err(|e| PassphraseLayerError::Crypto(e.to_string()))?;
    let mut bytes = Vec::with_capacity(MAGIC.len() + SALT_BYTES + sealed.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(salt);
    bytes.extend_from_slice(&sealed);
    Ok(bytes)
}

/// Opens a passphrase-sealed guardian backup, returning the guardian backup
/// the shares then open.
///
/// # Errors
/// [`PassphraseLayerError::NotSealed`] for other bytes,
/// [`PassphraseLayerError::WrongPassphrase`] when the passphrase does not
/// open them.
pub fn open(passphrase: &str, bytes: &[u8]) -> Result<Vec<u8>, PassphraseLayerError> {
    let rest = bytes
        .strip_prefix(MAGIC)
        .ok_or(PassphraseLayerError::NotSealed)?;
    let (salt, sealed) = rest
        .split_first_chunk::<SALT_BYTES>()
        .ok_or(PassphraseLayerError::NotSealed)?;
    let key = passphrase_key(passphrase, salt)?;
    decrypt(&key, sealed).map_err(|_| PassphraseLayerError::WrongPassphrase)
}
