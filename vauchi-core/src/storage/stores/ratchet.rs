// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Ratchet domain persistence view (contact_ratchets).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use rusqlite::{Connection, params};
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
    key: &'a SymmetricKey,
    receive_key: Option<&'a SymmetricKey>,
    clock: &'a Arc<dyn Clock>,
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
                key: &self.encryption_key,
                receive_key: self.receive_key(),
                clock: &self.clock,
            }
        })
    }
}

impl RatchetStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
    /// The key a new row is sealed with: the receive key once the storage
    /// has it, the storage key before.
    fn sealing_key(&self, contact_id: &str) -> (SymmetricKey, bool) {
        match self.receive_key {
            Some(receive_key) => (ratchet_key_under_receive_key(receive_key, contact_id), true),
            None => (ratchet_key_under_storage_key(self.key, contact_id), false),
        }
    }

    fn opening_key(
        &self,
        contact_id: &str,
        sealed_with_receive_key: bool,
    ) -> Result<SymmetricKey, StorageError> {
        match (sealed_with_receive_key, self.receive_key) {
            (false, _) => Ok(ratchet_key_under_storage_key(self.key, contact_id)),
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

        let (ratchet_key, sealed_with_receive_key) = self.sealing_key(contact_id);
        let state_encrypted = crate::crypto::encrypt(&ratchet_key, &state_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO contact_ratchets
             (contact_id, peer_device_id, ratchet_state_encrypted, is_initiator, updated_at,
              sealed_with_receive_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                contact_id,
                peer_device_id,
                state_encrypted,
                is_initiator as i32,
                now as i64,
                sealed_with_receive_key,
            ],
        )?;

        Ok(())
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
            "SELECT ratchet_state_encrypted, is_initiator, sealed_with_receive_key
             FROM contact_ratchets
             WHERE contact_id = ?1 AND peer_device_id = ?2",
            params![contact_id, peer_device_id],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i32>(1)? != 0,
                    row.get::<_, bool>(2)?,
                ))
            },
        );

        match result {
            Ok((encrypted, is_initiator, sealed_with_receive_key)) => {
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

                Ok(Some((state, is_initiator)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Deletes the ratchet session for one device belonging to a contact.
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
