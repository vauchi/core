// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage for persisted exchange state (crash recovery for Link mode).

use rusqlite::params;

use crate::exchange::exchange_id::ExchangeId;
use crate::exchange::persisted_state::PersistedExchangeState;
use crate::storage::Storage;
use crate::storage::error::StorageError;

impl Storage {
    /// Save or update a persisted exchange state.
    ///
    /// The entire state is serialized to JSON and encrypted under SEK
    /// (ADR-033). The exchange_id is stored as hex TEXT for lookups.
    pub fn save_exchange_state(&self, state: &PersistedExchangeState) -> Result<(), StorageError> {
        let json_bytes =
            serde_json::to_vec(state).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(&self.encryption_key, &json_bytes)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let expires_at = state
            .created_at
            .saturating_add(u64::from(state.ttl_seconds));

        self.conn.execute(
            "INSERT OR REPLACE INTO exchange_states \
             (exchange_id, encrypted_blob, created_at, expires_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                state.exchange_id.to_string(),
                encrypted,
                state.created_at as i64,
                expires_at as i64,
            ],
        )?;
        Ok(())
    }

    /// Load a persisted exchange state by its ID.
    pub fn load_exchange_state(
        &self,
        exchange_id: &ExchangeId,
    ) -> Result<Option<PersistedExchangeState>, StorageError> {
        let result = self.conn.query_row(
            "SELECT encrypted_blob FROM exchange_states WHERE exchange_id = ?1",
            params![exchange_id.to_string()],
            |row| row.get::<_, Vec<u8>>(0),
        );

        match result {
            Ok(encrypted) => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let state: PersistedExchangeState = serde_json::from_slice(&decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// List all active (non-expired) exchange states.
    ///
    /// Used at app launch to detect and resume interrupted exchanges.
    pub fn list_active_exchange_states(
        &self,
        now_secs: u64,
    ) -> Result<Vec<PersistedExchangeState>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT encrypted_blob FROM exchange_states WHERE expires_at > ?1")?;
        let rows = stmt.query_map(params![now_secs as i64], |row| row.get::<_, Vec<u8>>(0))?;

        let mut states = Vec::new();
        for encrypted in rows {
            let encrypted = encrypted?;
            let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let state: PersistedExchangeState = serde_json::from_slice(&decrypted)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            states.push(state);
        }
        Ok(states)
    }

    /// Delete a persisted exchange state (on completion or cancellation).
    pub fn delete_exchange_state(&self, exchange_id: &ExchangeId) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM exchange_states WHERE exchange_id = ?1",
            params![exchange_id.to_string()],
        )?;
        Ok(())
    }

    /// Delete all expired exchange states.
    ///
    /// Returns the number of states removed.
    pub fn sweep_expired_exchange_states(&self, now_secs: u64) -> Result<usize, StorageError> {
        let deleted = self.conn.execute(
            "DELETE FROM exchange_states WHERE expires_at <= ?1",
            params![now_secs as i64],
        )?;
        Ok(deleted)
    }
}
