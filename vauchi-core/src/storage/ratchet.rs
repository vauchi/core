//! Double Ratchet state storage operations.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::crypto::ratchet::DoubleRatchetState;

impl Storage {
    // === Double Ratchet State Operations ===

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

        // Encrypt the serialized state
        let state_encrypted = crate::crypto::encrypt(&self.encryption_key, &state_json)
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
                // Decrypt the state
                let state_json = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;

                // Deserialize
                let serialized: crate::crypto::ratchet::SerializedRatchetState =
                    serde_json::from_slice(&state_json)
                        .map_err(|e| StorageError::Serialization(e.to_string()))?;

                let state = DoubleRatchetState::deserialize(serialized)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;

                Ok(Some((state, is_initiator)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
}
