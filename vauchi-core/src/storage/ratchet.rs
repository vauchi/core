// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Double Ratchet state storage operations.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::crypto::SymmetricKey;
use crate::crypto::kdf::HKDF;
use crate::crypto::ratchet::DoubleRatchetState;

impl Storage {
    // === Double Ratchet State Operations ===

    /// Derives a per-contact encryption key for ratchet state storage.
    ///
    /// Uses HKDF with the storage master key as IKM and the contact ID
    /// as domain separator. This ensures that compromising one contact's
    /// ratchet state does not expose the SMK or other contacts' states.
    fn derive_ratchet_key(&self, contact_id: &str) -> SymmetricKey {
        let mut info = b"vauchi-ratchet-storage-v1:".to_vec();
        info.extend_from_slice(contact_id.as_bytes());
        let key_bytes = HKDF::derive_key(None, self.encryption_key.as_bytes(), &info);
        SymmetricKey::from_bytes(*key_bytes)
    }

    /// Saves a Double Ratchet state for a contact.
    pub fn save_ratchet_state(
        &self,
        contact_id: &str,
        state: &DoubleRatchetState,
        is_initiator: bool,
    ) -> Result<(), StorageError> {
        // Serialize the ratchet state
        let serialized = state.serialize();
        let state_json = serde_json::to_vec(&serialized)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        // Encrypt with per-contact derived key
        let ratchet_key = self.derive_ratchet_key(contact_id);
        let state_encrypted = crate::crypto::encrypt(&ratchet_key, &state_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO contact_ratchets
             (contact_id, ratchet_state_encrypted, is_initiator, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![contact_id, state_encrypted, is_initiator as i32, now as i64,],
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
        let result = self.conn.query_row(
            "SELECT ratchet_state_encrypted, is_initiator FROM contact_ratchets WHERE contact_id = ?1",
            params![contact_id],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i32>(1)? != 0,
                ))
            },
        );

        match result {
            Ok((encrypted, is_initiator)) => {
                // Decrypt with per-contact derived key
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
}
