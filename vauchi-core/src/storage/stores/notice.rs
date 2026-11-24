// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The notice core's saved state (`notice_states`, vauchi/private#1375).
//!
//! Each row is the `save()` bytes of one notice-core object: a direction's
//! sender or receiver for one peer device, or a contact's publisher or
//! pending store. It is sealed under a key of its contact, under the
//! Background Receive Key once the storage has it, as ratchet state is.
//! A notice state holds chain keys: if two connections both advanced one
//! direction, the second would reuse a chain position and the receiver
//! would alarm, so each row carries a version and a save from a connection
//! that read the row before another saved it is refused.

use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use crate::crypto::hkdf::HKDF;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::Arc;
use zeroize::Zeroizing;

use super::super::{Storage, StorageError};

/// Which notice-core object a row holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeStateKind {
    Sender,
    Receiver,
    Publisher,
    Pending,
}

impl NoticeStateKind {
    fn code(self) -> i64 {
        match self {
            Self::Sender => 1,
            Self::Receiver => 2,
            Self::Publisher => 3,
            Self::Pending => 4,
        }
    }
}

/// The per-contact key for notice state under the storage key.
pub(in crate::storage) fn notice_key_under_storage_key(
    storage_key: &SymmetricKey,
    contact_id: &str,
) -> SymmetricKey {
    per_contact_key(storage_key, b"vauchi-notice-storage-v1:", contact_id)
}

/// The per-contact key for notice state under the Background Receive Key.
pub(in crate::storage) fn notice_key_under_receive_key(
    receive_key: &SymmetricKey,
    contact_id: &str,
) -> SymmetricKey {
    per_contact_key(receive_key, b"vauchi-notice-receive-key-v1:", contact_id)
}

fn per_contact_key(root: &SymmetricKey, label: &[u8], contact_id: &str) -> SymmetricKey {
    let mut info = label.to_vec();
    info.extend_from_slice(contact_id.as_bytes());
    SymmetricKey::from_bytes(*HKDF::derive_key(None, root.as_bytes(), &info))
}

/// Scoped persistence view for the notice core's state.
pub struct NoticeStore<'a> {
    conn: &'a Connection,
    key: Option<&'a SymmetricKey>,
    receive_key: Option<&'a SymmetricKey>,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the notice core's state.
    pub fn notices(&self) -> crate::storage::Sealed<'_, NoticeStore<'_>> {
        self.sealed(NoticeStore {
            conn: &self.conn,
            key: Some(&self.encryption_key),
            receive_key: self.receive_key(),
            clock: &self.clock,
        })
    }
}

impl NoticeStore<'_> {
    /// Saves `state` for `contact_id`'s `kind` on `peer_device_id`.
    ///
    /// [`StorageError::NoticeMoved`] when another connection saved the row
    /// after this one last read or saved it; a row this connection never
    /// read is replaced.
    pub fn save(
        &self,
        contact_id: &str,
        kind: NoticeStateKind,
        peer_device_id: &[u8; 32],
        state: &[u8],
    ) -> Result<(), StorageError> {
        let (key, sealed_with_receive_key) = self.sealing_key(contact_id)?;
        let state_encrypted = crate::crypto::encrypt(&key, state)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let seen = self.version_seen(contact_id, kind, peer_device_id)?;
        let current: Option<i64> = self
            .conn
            .query_row(
                "SELECT row_version FROM notice_states
                 WHERE contact_id = ?1 AND kind = ?2 AND peer_device_id = ?3",
                params![contact_id, kind.code(), peer_device_id],
                |row| row.get(0),
            )
            .optional()?;
        if let (Some(current), Some(seen)) = (current, seen)
            && current != seen
        {
            return Err(StorageError::NoticeMoved);
        }
        let row_version: i64 = self.conn.query_row(
            "INSERT INTO notice_states
                 (contact_id, kind, peer_device_id, state_encrypted,
                  sealed_with_receive_key, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (contact_id, kind, peer_device_id) DO UPDATE SET
                 state_encrypted = excluded.state_encrypted,
                 sealed_with_receive_key = excluded.sealed_with_receive_key,
                 updated_at = excluded.updated_at,
                 row_version = notice_states.row_version + 1
             RETURNING row_version",
            params![
                contact_id,
                kind.code(),
                peer_device_id,
                state_encrypted,
                sealed_with_receive_key,
                self.clock.unix_seconds() as i64,
            ],
            |row| row.get(0),
        )?;
        self.remember_version(contact_id, kind, peer_device_id, row_version)
    }

    /// The state saved for `contact_id`'s `kind` on `peer_device_id`.
    pub fn load(
        &self,
        contact_id: &str,
        kind: NoticeStateKind,
        peer_device_id: &[u8; 32],
    ) -> Result<Option<Zeroizing<Vec<u8>>>, StorageError> {
        let row: Option<(Vec<u8>, bool, i64)> = self
            .conn
            .query_row(
                "SELECT state_encrypted, sealed_with_receive_key, row_version
                 FROM notice_states
                 WHERE contact_id = ?1 AND kind = ?2 AND peer_device_id = ?3",
                params![contact_id, kind.code(), peer_device_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((encrypted, sealed_with_receive_key, row_version)) = row else {
            return Ok(None);
        };
        let key = self.opening_key(contact_id, sealed_with_receive_key)?;
        let state = Zeroizing::new(
            crate::crypto::decrypt(&key, &encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?,
        );
        self.remember_version(contact_id, kind, peer_device_id, row_version)?;
        Ok(Some(state))
    }

    fn sealing_key(&self, contact_id: &str) -> Result<(SymmetricKey, bool), StorageError> {
        match (self.receive_key, self.key) {
            (Some(receive_key), _) => {
                Ok((notice_key_under_receive_key(receive_key, contact_id), true))
            }
            (None, Some(key)) => Ok((notice_key_under_storage_key(key, contact_id), false)),
            (None, None) => Err(StorageError::ReceiveKeyUnavailable),
        }
    }

    fn opening_key(
        &self,
        contact_id: &str,
        sealed_with_receive_key: bool,
    ) -> Result<SymmetricKey, StorageError> {
        match (sealed_with_receive_key, self.receive_key) {
            (false, _) => self
                .key
                .map(|key| notice_key_under_storage_key(key, contact_id))
                .ok_or(StorageError::StorageKeyUnavailable),
            (true, Some(receive_key)) => Ok(notice_key_under_receive_key(receive_key, contact_id)),
            (true, None) => Err(StorageError::ReceiveKeyUnavailable),
        }
    }

    fn ensure_versions_seen(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS notice_versions_seen (
                contact_id TEXT NOT NULL,
                kind INTEGER NOT NULL,
                peer_device_id BLOB NOT NULL,
                row_version INTEGER NOT NULL,
                PRIMARY KEY (contact_id, kind, peer_device_id)
            )",
        )?;
        Ok(())
    }

    fn version_seen(
        &self,
        contact_id: &str,
        kind: NoticeStateKind,
        peer_device_id: &[u8; 32],
    ) -> Result<Option<i64>, StorageError> {
        self.ensure_versions_seen()?;
        Ok(self
            .conn
            .query_row(
                "SELECT row_version FROM temp.notice_versions_seen
                 WHERE contact_id = ?1 AND kind = ?2 AND peer_device_id = ?3",
                params![contact_id, kind.code(), peer_device_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn remember_version(
        &self,
        contact_id: &str,
        kind: NoticeStateKind,
        peer_device_id: &[u8; 32],
        row_version: i64,
    ) -> Result<(), StorageError> {
        self.ensure_versions_seen()?;
        self.conn.execute(
            "INSERT INTO temp.notice_versions_seen VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (contact_id, kind, peer_device_id)
             DO UPDATE SET row_version = excluded.row_version",
            params![contact_id, kind.code(), peer_device_id, row_version],
        )?;
        Ok(())
    }
}
