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
    #[cfg(feature = "notice-core")]
    storage: &'a Storage,
    conn: &'a Connection,
    key: Option<&'a SymmetricKey>,
    receive_key: Option<&'a SymmetricKey>,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the notice core's state.
    pub fn notices(&self) -> crate::storage::Sealed<'_, NoticeStore<'_>> {
        self.sealed(NoticeStore {
            #[cfg(feature = "notice-core")]
            storage: self,
            conn: &self.conn,
            key: Some(&self.encryption_key),
            receive_key: self.receive_key(),
            clock: &self.clock,
        })
    }
}

impl NoticeStore<'_> {
    /// Advances every row of a session in one durable commit. A stale row or
    /// failed commit refuses the entire batch, leaving no partly advanced pair.
    #[cfg(feature = "notice-core")]
    pub fn save_many(&self, writes: &[NoticeWrite<'_>]) -> Result<(), StorageError> {
        self.storage.with_notice_transaction(|| {
            for write in writes {
                self.save(
                    write.contact_id,
                    write.kind,
                    &write.peer_device_id,
                    write.state,
                )?;
            }
            Ok(())
        })
    }

    /// Saves `state` for `contact_id`'s `kind` on `peer_device_id`.
    ///
    /// [`StorageError::NoticeMoved`] when another connection saved the row
    /// after this one last read or saved it. An unobserved row can only be
    /// created, never replaced.
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
        let (seen, previous) = self
            .version_seen(contact_id, kind, peer_device_id)?
            .unwrap_or((-1, Vec::new()));
        // The notices this state assigned leave after this save; a save a
        // power loss could undo would let the next session seal new
        // plaintexts under keys already used (ADR-019 amendment 2026-11-24).
        let _durable = crate::storage::full_sync::FullSync::full(self.conn)?;
        let row_version: i64 = self
            .conn
            .query_row(
                "INSERT INTO notice_states
                 (contact_id, kind, peer_device_id, state_encrypted,
                  sealed_with_receive_key, updated_at)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6
             WHERE COALESCE(
                 (SELECT row_version FROM notice_states
                  WHERE contact_id = ?1 AND kind = ?2 AND peer_device_id = ?3),
                 -1) = ?7 AND (?7 = -1 OR
                 (SELECT state_encrypted FROM notice_states
                  WHERE contact_id = ?1 AND kind = ?2 AND peer_device_id = ?3) = ?8)
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
                    seen,
                    previous,
                ],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StorageError::NoticeMoved)?;
        self.remember_version(
            contact_id,
            kind,
            peer_device_id,
            row_version,
            &state_encrypted,
        )
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
            self.remember_version(contact_id, kind, peer_device_id, -1, &[])?;
            return Ok(None);
        };
        let key = self.opening_key(contact_id, sealed_with_receive_key)?;
        let state = Zeroizing::new(
            crate::crypto::decrypt(&key, &encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?,
        );
        self.remember_version(contact_id, kind, peer_device_id, row_version, &encrypted)?;
        Ok(Some(state))
    }

    /// Opens and saves the directions `contact_id` lacks with each of `peers`,
    /// and returns how many devices it opened. A saved direction is never
    /// opened again: that would restart its chain under keys it already used.
    #[cfg(feature = "notice-core")]
    pub fn ensure_directions(
        &self,
        contact_id: &str,
        shared_key: &[u8; 32],
        own_device: &crate::crypto::X25519KeyPair,
        peers: &[crate::notice::pairing::PeerDevice],
        now: u64,
    ) -> Result<usize, StorageError> {
        let mut opened = 0;
        for peer in peers {
            let has = |kind| -> Result<bool, StorageError> {
                Ok(self.load(contact_id, kind, &peer.device_id)?.is_some())
            };
            let (has_sender, has_receiver) = (
                has(NoticeStateKind::Sender)?,
                has(NoticeStateKind::Receiver)?,
            );
            if has_sender && has_receiver {
                continue;
            }
            // Opening is deterministic from the shared key, so a direction a
            // crash left unsaved is opened again exactly as it would have been.
            let (sender, receiver) = crate::notice::pairing::open_directions(
                shared_key,
                own_device,
                &peer.exchange_public,
                now,
            );
            if !has_sender {
                let sender = sender.save().map_err(saved_notice_error)?;
                self.save(
                    contact_id,
                    NoticeStateKind::Sender,
                    &peer.device_id,
                    &sender,
                )?;
            }
            if !has_receiver {
                let receiver = receiver.save().map_err(saved_notice_error)?;
                self.save(
                    contact_id,
                    NoticeStateKind::Receiver,
                    &peer.device_id,
                    &receiver,
                )?;
            }
            opened += 1;
        }
        Ok(opened)
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
                state_encrypted BLOB NOT NULL,
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
    ) -> Result<Option<(i64, Vec<u8>)>, StorageError> {
        self.ensure_versions_seen()?;
        Ok(self
            .conn
            .query_row(
                "SELECT row_version, state_encrypted FROM temp.notice_versions_seen
                 WHERE contact_id = ?1 AND kind = ?2 AND peer_device_id = ?3",
                params![contact_id, kind.code(), peer_device_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    fn remember_version(
        &self,
        contact_id: &str,
        kind: NoticeStateKind,
        peer_device_id: &[u8; 32],
        row_version: i64,
        state_encrypted: &[u8],
    ) -> Result<(), StorageError> {
        self.ensure_versions_seen()?;
        self.conn.execute(
            "INSERT INTO temp.notice_versions_seen VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (contact_id, kind, peer_device_id)
             DO UPDATE SET row_version = excluded.row_version,
                 state_encrypted = excluded.state_encrypted",
            params![
                contact_id,
                kind.code(),
                peer_device_id,
                row_version,
                state_encrypted
            ],
        )?;
        Ok(())
    }
}

/// One serialized notice state in a session's atomic write set.
#[cfg(feature = "notice-core")]
pub struct NoticeWrite<'a> {
    contact_id: &'a str,
    kind: NoticeStateKind,
    peer_device_id: [u8; 32],
    state: &'a [u8],
}

#[cfg(feature = "notice-core")]
impl<'a> NoticeWrite<'a> {
    pub fn new(
        contact_id: &'a str,
        kind: NoticeStateKind,
        peer_device_id: [u8; 32],
        state: &'a [u8],
    ) -> Self {
        Self {
            contact_id,
            kind,
            peer_device_id,
            state,
        }
    }
}

#[cfg(feature = "notice-core")]
fn saved_notice_error(error: crate::notice::saved::SavedError) -> StorageError {
    StorageError::Serialization(format!("notice state: {error:?}"))
}
