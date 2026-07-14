// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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

/// Device ID used by peers that predate per-device ratchet sessions.
pub const LEGACY_PEER_DEVICE_ID: [u8; 32] = [0; 32];

/// Scoped persistence view for the ratchets domain.
pub struct RatchetStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the ratchets domain.
    pub fn ratchets(&self) -> RatchetStore<'_> {
        RatchetStore {
            conn: &self.conn,
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

impl RatchetStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
    /// Derives a per-contact encryption key for ratchet state storage.
    ///
    /// Uses HKDF with the storage master key as IKM and the contact ID
    /// as domain separator. This ensures that compromising one contact's
    /// ratchet state does not expose the SMK or other contacts' states.
    fn derive_ratchet_key(&self, contact_id: &str) -> SymmetricKey {
        let mut info = b"vauchi-ratchet-storage-v1:".to_vec();
        info.extend_from_slice(contact_id.as_bytes());
        let key_bytes = HKDF::derive_key(None, self.key.as_bytes(), &info);
        SymmetricKey::from_bytes(*key_bytes)
    }
    /// Saves a Double Ratchet state for a contact.
    pub fn save_ratchet_state(
        &self,
        contact_id: &str,
        state: &DoubleRatchetState,
        is_initiator: bool,
    ) -> Result<(), StorageError> {
        self.save_ratchet_state_for_device(contact_id, &LEGACY_PEER_DEVICE_ID, state, is_initiator)
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

        let ratchet_key = self.derive_ratchet_key(contact_id);
        let state_encrypted = crate::crypto::encrypt(&ratchet_key, &state_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO contact_ratchets
             (contact_id, peer_device_id, ratchet_state_encrypted, is_initiator, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                contact_id,
                peer_device_id,
                state_encrypted,
                is_initiator as i32,
                now as i64,
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
        self.load_ratchet_state_for_device(contact_id, &LEGACY_PEER_DEVICE_ID)
    }

    /// Loads the Double Ratchet state for one device belonging to a contact.
    pub fn load_ratchet_state_for_device(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
    ) -> Result<Option<(DoubleRatchetState, bool)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT ratchet_state_encrypted, is_initiator
             FROM contact_ratchets
             WHERE contact_id = ?1 AND peer_device_id = ?2",
            params![contact_id, peer_device_id],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i32>(1)? != 0)),
        );

        match result {
            Ok((encrypted, is_initiator)) => {
                let ratchet_key = self.derive_ratchet_key(contact_id);
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
