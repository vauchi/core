// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Version 6 binds its clear version/salt header with AEAD associated data.

#![cfg_attr(
    not(test),
    deny(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )
)]

use zeroize::Zeroizing;

use crate::crypto::{
    HKDF, SymmetricKey, decrypt_with_ad, derive_key_argon2id, encrypt_with_ad, random_bytes,
};

use super::super::full_backup::{
    MAX_DECOMPRESSED_BACKUP_BYTES, compress_backup, decompress_backup,
};
use super::super::{BackupError, MAX_FULL_BACKUP_BYTES, import_full_backup};
use super::BackupSnapshot;

pub(crate) const SNAPSHOT_VERSION: u8 = 6;
const HEADER_LENGTH: usize = 17;
const HKDF_INFO: &[u8] = b"vauchi-backup-v6-state";

fn derive_key(password: &str, salt: &[u8; 16]) -> Result<SymmetricKey, BackupError> {
    let base =
        derive_key_argon2id(password.as_bytes(), salt).map_err(|_| BackupError::KeyDerivation)?;
    let prk = Zeroizing::new(HKDF::extract(None, base.as_bytes()));
    let expanded =
        Zeroizing::new(HKDF::expand(&prk, HKDF_INFO, 32).map_err(|_| BackupError::KeyDerivation)?);
    let bytes = Zeroizing::new(
        expanded
            .as_slice()
            .try_into()
            .map_err(|_| BackupError::KeyDerivation)?,
    );
    SymmetricKey::try_from_bytes(*bytes).map_err(|_| BackupError::KeyDerivation)
}

pub(crate) fn encode(snapshot: &BackupSnapshot, password: &str) -> Result<Vec<u8>, BackupError> {
    let plaintext = Zeroizing::new(
        serde_json::to_vec(snapshot)
            .map_err(|_| BackupError::Serialization("Could not encode backup state".into()))?,
    );
    if plaintext.len() > MAX_DECOMPRESSED_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    let compressed = Zeroizing::new(compress_backup(&plaintext)?);
    let salt: [u8; 16] = random_bytes();
    let mut header = vec![SNAPSHOT_VERSION];
    header.extend(salt);
    let key = derive_key(password, &salt)?;
    let ciphertext = encrypt_with_ad(&key, &compressed, &header)
        .map_err(|_| BackupError::EncryptionFailed("Could not seal backup state".into()))?;
    header.extend(ciphertext);
    if header.len() > MAX_FULL_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    Ok(header)
}

pub(crate) fn decode(data: &[u8], password: &str) -> Result<BackupSnapshot, BackupError> {
    if data.len() > MAX_FULL_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    let (header, ciphertext) = data
        .split_first_chunk::<HEADER_LENGTH>()
        .ok_or(BackupError::TooShort)?;
    if header[0] != SNAPSHOT_VERSION {
        return BackupSnapshot::from_legacy(import_full_backup(data, password)?);
    }
    let salt = header[1..].try_into().map_err(|_| BackupError::TooShort)?;
    let key = derive_key(password, &salt)?;
    let decrypted = Zeroizing::new(
        decrypt_with_ad(&key, ciphertext, header).map_err(|_| BackupError::DecryptionFailed)?,
    );
    let plaintext = Zeroizing::new(decompress_backup(&decrypted)?);
    let snapshot: BackupSnapshot = serde_json::from_slice(&plaintext)
        .map_err(|_| BackupError::Deserialization("Invalid backup state".into()))?;
    if snapshot.version != u32::from(SNAPSHOT_VERSION) {
        return Err(BackupError::Deserialization(
            "Invalid backup state version".into(),
        ));
    }
    Ok(snapshot)
}
