// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Receive registrations stored ahead under the Background Receive Key, so
//! a locked phone can fetch without a contact's shared key or the master
//! seed (owner 2026-11-08, vauchi/private#1093 slice 5).
//!
//! The store keeps each day as sealed bytes. What a day holds belongs to
//! the receive path, so storage does not depend on network (VRS06).

use rusqlite::{Connection, OptionalExtension, params};
use zeroize::Zeroizing;

use crate::crypto::{SymmetricKey, decrypt, encrypt, kdf::HKDF};

use super::super::{Storage, StorageError};

/// Scoped persistence view for the receive registrations stored ahead.
pub struct ReceiveTokenStore<'a> {
    conn: &'a Connection,
    receive_key: Option<&'a SymmetricKey>,
}

impl Storage {
    /// Scoped persistence view for the receive registrations stored ahead.
    pub fn receive_tokens(&self) -> crate::storage::Sealed<'_, ReceiveTokenStore<'_>> {
        self.sealed(ReceiveTokenStore {
            conn: &self.conn,
            receive_key: self.receive_key(),
        })
    }
}

fn day_key(receive_key: &SymmetricKey, day: u64) -> SymmetricKey {
    let mut info = b"vauchi-receive-tokens-v1:".to_vec();
    info.extend_from_slice(&day.to_be_bytes());
    SymmetricKey::from_bytes(*HKDF::derive_key(None, receive_key.as_bytes(), &info))
}

impl ReceiveTokenStore<'_> {
    fn key(&self) -> Result<&SymmetricKey, StorageError> {
        self.receive_key.ok_or(StorageError::ReceiveKeyUnavailable)
    }

    /// The registration stored for `day`, if any.
    pub fn load_day(&self, day: u64) -> Result<Option<Zeroizing<Vec<u8>>>, StorageError> {
        let sealed: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT registration_encrypted FROM receive_tokens_ahead WHERE day = ?1",
                params![day as i64],
                |row| row.get(0),
            )
            .optional()?;
        let Some(sealed) = sealed else {
            return Ok(None);
        };
        decrypt(&day_key(self.key()?, day), &sealed)
            .map(|plain| Some(Zeroizing::new(plain)))
            .map_err(|e| StorageError::Encryption(e.to_string()))
    }

    /// Replaces the stored days with `days`, dropping every day before the
    /// first one, in one transaction.
    pub(crate) fn replace_days(
        &self,
        days: &[(u64, Zeroizing<Vec<u8>>)],
    ) -> Result<(), StorageError> {
        let key = self.key()?;
        let Some(first) = days.iter().map(|(day, _)| *day).min() else {
            return Ok(());
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM receive_tokens_ahead WHERE day < ?1",
            params![first as i64],
        )?;
        for (day, registration) in days {
            let sealed = encrypt(&day_key(key, *day), registration)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            tx.execute(
                "INSERT OR REPLACE INTO receive_tokens_ahead (day, registration_encrypted)
                 VALUES (?1, ?2)",
                params![*day as i64, sealed],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
