// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Ratchet domain persistence view (contact_ratchets).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).
//!
//! A locked run and the unlocked app are separate connections that can hold
//! the same session at once (vauchi/private#1093). Each row carries a
//! version, and each connection remembers in a temporary table the version
//! it last read or wrote; a save is refused when the row has moved past it.
//! The temporary table lives and rolls back with the connection's own
//! transactions, so a rolled-back save does not leave it ahead of the row.

use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::Arc;

use super::super::{Storage, StorageError};
use crate::crypto::kdf::HKDF;
use crate::crypto::ratchet::DoubleRatchetState;

/// Device ID of the shared per-contact session: genesis cold-start sends,
/// handshake ACKs, and peers whose sending device is not yet known (before
/// F4 activation). Not compatibility code; it stays (vauchi/private#575).
pub const SHARED_PEER_DEVICE_ID: [u8; 32] = [0; 32];

/// Scoped persistence view for the ratchets domain.
pub struct RatchetStore<'a> {
    conn: &'a Connection,
    key: Option<&'a SymmetricKey>,
    receive_key: Option<&'a SymmetricKey>,
    clock: &'a Arc<dyn Clock>,
}

impl<'a> RatchetStore<'a> {
    /// The view a locked run has: the receive key and no storage key.
    pub(in crate::storage) fn under_receive_key(
        conn: &'a Connection,
        receive_key: &'a SymmetricKey,
        clock: &'a Arc<dyn Clock>,
    ) -> Self {
        Self {
            conn,
            key: None,
            receive_key: Some(receive_key),
            clock,
        }
    }
}

/// The per-contact key for ratchet state under the storage key, so that
/// compromising one contact's state exposes neither the storage key nor
/// another contact's state.
pub(in crate::storage) fn ratchet_key_under_storage_key(
    storage_key: &SymmetricKey,
    contact_id: &str,
) -> SymmetricKey {
    per_contact_key(storage_key, b"vauchi-ratchet-storage-v1:", contact_id)
}

/// The per-contact key for ratchet state under the Background Receive Key
/// (vauchi/private#1093).
pub(in crate::storage) fn ratchet_key_under_receive_key(
    receive_key: &SymmetricKey,
    contact_id: &str,
) -> SymmetricKey {
    per_contact_key(receive_key, b"vauchi-ratchet-receive-key-v1:", contact_id)
}

fn per_contact_key(root: &SymmetricKey, label: &[u8], contact_id: &str) -> SymmetricKey {
    let mut info = label.to_vec();
    info.extend_from_slice(contact_id.as_bytes());
    SymmetricKey::from_bytes(*HKDF::derive_key(None, root.as_bytes(), &info))
}

impl Storage {
    /// Scoped persistence view for the ratchets domain.
    pub fn ratchets(&self) -> crate::storage::Sealed<'_, RatchetStore<'_>> {
        self.sealed({
            RatchetStore {
                conn: &self.conn,
                key: Some(&self.encryption_key),
                receive_key: self.receive_key(),
                clock: &self.clock,
            }
        })
    }
}

/// A sealed session as a save writes it.
struct SealedRow<'r> {
    state_encrypted: &'r [u8],
    is_initiator: bool,
    sealed_with_receive_key: bool,
    updated_at: u64,
}

/// What a save did to the row it targets.
enum SaveOutcome {
    Saved(i64),
    Moved,
}

impl RatchetStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }

    fn ensure_versions_seen(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS ratchet_versions_seen (
                contact_id TEXT NOT NULL,
                peer_device_id BLOB NOT NULL,
                row_version INTEGER NOT NULL,
                PRIMARY KEY (contact_id, peer_device_id)
            )",
        )?;
        Ok(())
    }

    fn version_seen(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
    ) -> Result<Option<i64>, StorageError> {
        self.ensure_versions_seen()?;
        Ok(self
            .conn
            .query_row(
                "SELECT row_version FROM temp.ratchet_versions_seen
                 WHERE contact_id = ?1 AND peer_device_id = ?2",
                params![contact_id, peer_device_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn remember_version(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
        row_version: i64,
    ) -> Result<(), StorageError> {
        self.ensure_versions_seen()?;
        self.conn.execute(
            "INSERT OR REPLACE INTO temp.ratchet_versions_seen
             (contact_id, peer_device_id, row_version) VALUES (?1, ?2, ?3)",
            params![contact_id, peer_device_id, row_version],
        )?;
        Ok(())
    }

    /// Writes the row when it is still at `seen`, or whatever is there when
    /// this connection has seen no version of it.
    fn write_row(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
        row: &SealedRow<'_>,
        seen: Option<i64>,
    ) -> Result<SaveOutcome, StorageError> {
        if let Some(seen) = seen {
            let updated = self.conn.execute(
                "UPDATE contact_ratchets
                 SET ratchet_state_encrypted = ?3, is_initiator = ?4, updated_at = ?5,
                     sealed_with_receive_key = ?6, row_version = ?7 + 1
                 WHERE contact_id = ?1 AND peer_device_id = ?2 AND row_version = ?7",
                params![
                    contact_id,
                    peer_device_id,
                    row.state_encrypted,
                    row.is_initiator as i32,
                    row.updated_at as i64,
                    row.sealed_with_receive_key,
                    seen,
                ],
            )?;
            if updated == 1 {
                return Ok(SaveOutcome::Saved(seen + 1));
            }
            let still_there: Option<i64> = self
                .conn
                .query_row(
                    "SELECT 1 FROM contact_ratchets WHERE contact_id = ?1 AND peer_device_id = ?2",
                    params![contact_id, peer_device_id],
                    |found| found.get(0),
                )
                .optional()?;
            if still_there.is_some() {
                return Ok(SaveOutcome::Moved);
            }
        }
        let row_version = self.conn.query_row(
            "INSERT INTO contact_ratchets
             (contact_id, peer_device_id, ratchet_state_encrypted, is_initiator, updated_at,
              sealed_with_receive_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (contact_id, peer_device_id) DO UPDATE SET
                 ratchet_state_encrypted = excluded.ratchet_state_encrypted,
                 is_initiator = excluded.is_initiator,
                 updated_at = excluded.updated_at,
                 sealed_with_receive_key = excluded.sealed_with_receive_key,
                 row_version = contact_ratchets.row_version + 1
             RETURNING row_version",
            params![
                contact_id,
                peer_device_id,
                row.state_encrypted,
                row.is_initiator as i32,
                row.updated_at as i64,
                row.sealed_with_receive_key,
            ],
            |written| written.get(0),
        )?;
        Ok(SaveOutcome::Saved(row_version))
    }
    /// The key a new row is sealed with: the receive key once the storage
    /// has it, the storage key before.
    fn sealing_key(&self, contact_id: &str) -> Result<(SymmetricKey, bool), StorageError> {
        match (self.receive_key, self.key) {
            (Some(receive_key), _) => {
                Ok((ratchet_key_under_receive_key(receive_key, contact_id), true))
            }
            (None, Some(key)) => Ok((ratchet_key_under_storage_key(key, contact_id), false)),
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
                .map(|key| ratchet_key_under_storage_key(key, contact_id))
                .ok_or(StorageError::StorageKeyUnavailable),
            (true, Some(receive_key)) => Ok(ratchet_key_under_receive_key(receive_key, contact_id)),
            (true, None) => Err(StorageError::ReceiveKeyUnavailable),
        }
    }
    /// Saves a Double Ratchet state for a contact.
    pub fn save_ratchet_state(
        &self,
        contact_id: &str,
        state: &DoubleRatchetState,
        is_initiator: bool,
    ) -> Result<(), StorageError> {
        self.save_ratchet_state_for_device(contact_id, &SHARED_PEER_DEVICE_ID, state, is_initiator)
    }

    /// Saves a Double Ratchet state for one device belonging to a contact.
    ///
    /// [`StorageError::RatchetMoved`] when another connection saved the
    /// session after this one last read or saved it; a session this
    /// connection never read is replaced.
    pub fn save_ratchet_state_for_device(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
        state: &DoubleRatchetState,
        is_initiator: bool,
    ) -> Result<(), StorageError> {
        let serialized = state.serialize();
        let state_json = serde_json::to_vec(&serialized)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let (ratchet_key, sealed_with_receive_key) = self.sealing_key(contact_id)?;
        let state_encrypted = crate::crypto::encrypt(&ratchet_key, &state_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let seen = self.version_seen(contact_id, peer_device_id)?;
        let row = SealedRow {
            state_encrypted: &state_encrypted,
            is_initiator,
            sealed_with_receive_key,
            updated_at: self.now_secs(),
        };
        match self.write_row(contact_id, peer_device_id, &row, seen)? {
            SaveOutcome::Saved(row_version) => {
                self.remember_version(contact_id, peer_device_id, row_version)
            }
            SaveOutcome::Moved => Err(StorageError::RatchetMoved),
        }
    }
    /// Loads a Double Ratchet state for a contact.
    ///
    /// Returns the ratchet state and whether this side was the initiator.
    pub fn load_ratchet_state(
        &self,
        contact_id: &str,
    ) -> Result<Option<(DoubleRatchetState, bool)>, StorageError> {
        self.load_ratchet_state_for_device(contact_id, &SHARED_PEER_DEVICE_ID)
    }

    /// Loads the Double Ratchet state for one device belonging to a contact.
    pub fn load_ratchet_state_for_device(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
    ) -> Result<Option<(DoubleRatchetState, bool)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT ratchet_state_encrypted, is_initiator, sealed_with_receive_key, row_version
             FROM contact_ratchets
             WHERE contact_id = ?1 AND peer_device_id = ?2",
            params![contact_id, peer_device_id],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i32>(1)? != 0,
                    row.get::<_, bool>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        );

        match result {
            Ok((encrypted, is_initiator, sealed_with_receive_key, row_version)) => {
                let ratchet_key = self.opening_key(contact_id, sealed_with_receive_key)?;
                // F6 audit fix: wrap in Zeroizing — contains root_key, DH secrets, chain keys
                let mut state_json = crate::crypto::decrypt(&ratchet_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;

                // Deserialize, then zeroize the JSON buffer
                let serialized: crate::crypto::ratchet::SerializedRatchetState =
                    serde_json::from_slice(&state_json)
                        .map_err(|e| StorageError::Serialization(e.to_string()))?;
                zeroize::Zeroize::zeroize(&mut state_json);

                let state = DoubleRatchetState::deserialize(serialized)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                self.remember_version(contact_id, peer_device_id, row_version)?;

                Ok(Some((state, is_initiator)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// The devices `contact_id` has a session with, the shared session
    /// first: its id is all zeros.
    pub fn peer_devices(&self, contact_id: &str) -> Result<Vec<[u8; 32]>, StorageError> {
        let devices: Vec<Vec<u8>> = self
            .conn
            .prepare(
                "SELECT peer_device_id FROM contact_ratchets
                 WHERE contact_id = ?1 ORDER BY peer_device_id",
            )?
            .query_map(params![contact_id], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(devices
            .into_iter()
            .filter_map(|device| device.try_into().ok())
            .collect())
    }

    /// Deletes every ratchet state of `contact_id`, on all its devices.
    pub fn delete_ratchet_states_for_contact(
        &self,
        contact_id: &str,
    ) -> Result<usize, StorageError> {
        Ok(self.conn.execute(
            "DELETE FROM contact_ratchets WHERE contact_id = ?1",
            rusqlite::params![contact_id],
        )?)
    }

    pub fn delete_ratchet_state_for_device(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
    ) -> Result<bool, StorageError> {
        let deleted = self.conn.execute(
            "DELETE FROM contact_ratchets
             WHERE contact_id = ?1 AND peer_device_id = ?2",
            params![contact_id, peer_device_id],
        )?;
        Ok(deleted > 0)
    }
    /// Deletes every stored ratchet session, returning how many were removed.
    ///
    /// Decommission path for a replaced device: without sessions the send
    /// loop skips all contacts, so this device can no longer advance a
    /// chain its replacement now owns.
    pub fn delete_all_ratchet_states(&self) -> Result<usize, StorageError> {
        let deleted = self.conn.execute("DELETE FROM contact_ratchets", [])?;
        Ok(deleted)
    }
}
