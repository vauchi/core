// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Proves at open that the storage key is the key the data is encrypted
//! with (vauchi/private#580).
//!
//! The `key_check` row seals a fixed label under a key derived from the
//! storage key. A database written before the row existed is proven by its
//! identity row instead, and only then gets a row — so a wrong key is never
//! recorded as the database's key.

use rusqlite::{Connection, OptionalExtension, params};

use crate::crypto::{SymmetricKey, decrypt, encrypt, kdf::HKDF};

use super::StorageError;

const KEY_CHECK_INFO: &[u8] = b"Vauchi_Storage_Key_Check_v1";
const KEY_CHECK_LABEL: &[u8] = b"vauchi storage key check";
const MAX_SEALED_LEN: usize = 256;

fn check_key(storage_key: &SymmetricKey) -> SymmetricKey {
    let derived = HKDF::derive_key(None, storage_key.as_bytes(), KEY_CHECK_INFO);
    SymmetricKey::from_bytes(*derived)
}

fn opens(storage_key: &SymmetricKey, sealed: &[u8]) -> bool {
    sealed.len() <= MAX_SEALED_LEN
        && decrypt(&check_key(storage_key), sealed).is_ok_and(|label| label == KEY_CHECK_LABEL)
}

fn seal(storage_key: &SymmetricKey) -> Result<Vec<u8>, StorageError> {
    encrypt(&check_key(storage_key), KEY_CHECK_LABEL)
        .map_err(|e| StorageError::Encryption(e.to_string()))
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, StorageError> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// Fails with [`StorageError::WrongKey`] unless `storage_key` opens the
/// data. Runs before migrations, so none of them runs under a wrong key.
pub(super) fn verify(conn: &Connection, storage_key: &SymmetricKey) -> Result<(), StorageError> {
    if table_exists(conn, "key_check")? {
        let sealed: Option<Vec<u8>> = conn
            .query_row(
                "SELECT sealed_label FROM key_check WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(sealed) = sealed {
            return if opens(storage_key, &sealed) {
                Ok(())
            } else {
                Err(StorageError::WrongKey)
            };
        }
    }
    verify_against_identity(conn, storage_key)
}

fn verify_against_identity(
    conn: &Connection,
    storage_key: &SymmetricKey,
) -> Result<(), StorageError> {
    if !table_exists(conn, "identity")? {
        return Ok(());
    }
    let backup: Option<Vec<u8>> = conn
        .query_row(
            "SELECT backup_data_encrypted FROM identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match backup {
        Some(backup) if decrypt(storage_key, &backup).is_err() => Err(StorageError::WrongKey),
        _ => Ok(()),
    }
}

/// Records the check for a database that has none; call only after
/// [`verify`] passed.
pub(super) fn record_if_missing(
    conn: &Connection,
    storage_key: &SymmetricKey,
) -> Result<(), StorageError> {
    conn.execute(
        "INSERT OR IGNORE INTO key_check (id, sealed_label) VALUES (1, ?1)",
        params![seal(storage_key)?],
    )?;
    Ok(())
}

/// Replaces the check; called inside the rekey transaction so the check and
/// the data move to the new key together.
pub(super) fn rewrite(conn: &Connection, storage_key: &SymmetricKey) -> Result<(), StorageError> {
    conn.execute(
        "INSERT OR REPLACE INTO key_check (id, sealed_label) VALUES (1, ?1)",
        params![seal(storage_key)?],
    )?;
    Ok(())
}
