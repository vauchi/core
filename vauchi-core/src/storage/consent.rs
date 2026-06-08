// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Consent storage operations.

use rusqlite::params;

use super::{Storage, StorageError};

impl Storage {
    /// Inserts or updates a consent record.
    pub fn execute_consent_upsert(
        &self,
        id: &str,
        consent_type: &str,
        granted: bool,
        timestamp: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO consent_records (id, consent_type, granted, timestamp)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, consent_type, granted as i32, timestamp as i64],
        )?;
        Ok(())
    }

    /// Checks if consent is granted for a type (latest record).
    pub fn check_consent(&self, consent_type: &str) -> Result<bool, StorageError> {
        let result = self.conn.query_row(
            "SELECT granted FROM consent_records WHERE consent_type = ?1 ORDER BY timestamp DESC, rowid DESC LIMIT 1",
            params![consent_type],
            |row| row.get::<_, i32>(0),
        );

        match result {
            Ok(granted) => Ok(granted != 0),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Lists all consent records as tuples of (id, consent_type, granted, timestamp).
    ///
    /// Returns raw tuples to avoid circular dependency with the api::consent module.
    pub fn list_consent_records(&self) -> Result<Vec<(String, String, bool, u64)>, StorageError> {
        // Handle missing table gracefully (pre-migration) (#60)
        let mut stmt = match self.conn.prepare(
            "SELECT id, consent_type, granted, timestamp FROM consent_records ORDER BY timestamp",
        ) {
            Ok(stmt) => stmt,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ffi::ErrorCode::Unknown || err.extended_code == 1 =>
            {
                return Ok(Vec::new());
            }
            Err(e) => return Err(StorageError::Database(e)),
        };

        let records = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(id, ct_str, granted, ts)| (id, ct_str, granted != 0, ts as u64))
            .collect();

        Ok(records)
    }

    /// Saves a consent record with policy version.
    pub fn execute_consent_upsert_with_version(
        &self,
        id: &str,
        consent_type: &str,
        granted: bool,
        timestamp: u64,
        policy_version: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO consent_records (id, consent_type, granted, timestamp, policy_version)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, consent_type, granted as i32, timestamp as i64, policy_version],
        )?;
        Ok(())
    }

    /// Lists all consent records including policy version.
    ///
    /// Returns tuples of (id, consent_type, granted, timestamp, policy_version).
    #[allow(clippy::type_complexity)]
    pub fn list_consent_records_with_version(
        &self,
    ) -> Result<Vec<(String, String, bool, u64, Option<String>)>, StorageError> {
        // Handle missing table gracefully (pre-migration) (#60)
        let mut stmt = match self.conn.prepare(
            "SELECT id, consent_type, granted, timestamp, policy_version
             FROM consent_records ORDER BY timestamp",
        ) {
            Ok(stmt) => stmt,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ffi::ErrorCode::Unknown || err.extended_code == 1 =>
            {
                return Ok(Vec::new());
            }
            Err(e) => return Err(StorageError::Database(e)),
        };

        let records = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(id, ct_str, granted, ts, pv)| (id, ct_str, granted != 0, ts as u64, pv))
            .collect();

        Ok(records)
    }

    /// Saves the deletion state (encrypted).
    pub fn save_deletion_state(&self, state: &super::DeletionState) -> Result<(), StorageError> {
        let json =
            serde_json::to_string(state).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(&self.encryption_key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO deletion_state (id, state_json, state_json_encrypted, updated_at) VALUES (1, '', ?1, ?2)",
            params![encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Loads the deletion state (decrypted).
    pub fn load_deletion_state(&self) -> Result<super::DeletionState, StorageError> {
        let result = self.conn.query_row(
            "SELECT state_json_encrypted, state_json FROM deletion_state WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let state = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(state)
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => {
                let state = serde_json::from_str(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(state)
            }
            Ok(_) => Ok(super::DeletionState::None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(super::DeletionState::None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Lists all audit log entries, decrypting details where applicable.
    ///
    /// Returns tuples of (event_type, details, timestamp).
    /// Encrypted details are decrypted with the storage key; falls back to
    /// plaintext `details` column for pre-encryption entries.
    pub fn list_audit_log(&self) -> Result<Vec<(String, Option<String>, u64)>, StorageError> {
        // Handle missing table gracefully (pre-migration) (#60)
        let mut stmt = match self.conn.prepare(
            "SELECT event_type, details_encrypted, details, timestamp FROM audit_log ORDER BY timestamp",
        ) {
            Ok(stmt) => stmt,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ffi::ErrorCode::Unknown
                    || err.extended_code == 1 =>
            {
                return Ok(Vec::new());
            }
            Err(e) => return Err(StorageError::Database(e)),
        };

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut entries = Vec::with_capacity(rows.len());
        for (event_type, details_encrypted, details_plain, ts) in rows {
            let details = if let Some(encrypted) = details_encrypted {
                if !encrypted.is_empty() {
                    let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                        .map_err(|e| StorageError::Encryption(e.to_string()))?;
                    Some(
                        String::from_utf8(decrypted)
                            .map_err(|e| StorageError::Serialization(e.to_string()))?,
                    )
                } else {
                    details_plain.filter(|s| !s.is_empty())
                }
            } else {
                details_plain.filter(|s| !s.is_empty())
            };
            entries.push((event_type, details, ts as u64));
        }

        Ok(entries)
    }

    /// Logs an audit event (details encrypted if present).
    pub fn log_audit_event(
        &self,
        event_type: &str,
        details: Option<&str>,
    ) -> Result<(), StorageError> {
        let now = self.now_secs();

        let details_encrypted = if let Some(d) = details {
            Some(
                crate::crypto::encrypt(&self.encryption_key, d.as_bytes())
                    .map_err(|e| StorageError::Encryption(e.to_string()))?,
            )
        } else {
            None
        };

        self.conn.execute(
            "INSERT INTO audit_log (event_type, details, details_encrypted, timestamp) VALUES (?1, '', ?2, ?3)",
            params![event_type, details_encrypted, now as i64],
        )?;
        Ok(())
    }
}
