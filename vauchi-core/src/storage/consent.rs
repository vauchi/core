// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Consent storage operations.

use rusqlite::params;

use super::{Storage, StorageError};

impl Storage {
    // === Consent Operations ===

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
            "SELECT granted FROM consent_records WHERE consent_type = ?1 ORDER BY timestamp DESC LIMIT 1",
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
        // Return empty vec if the table doesn't exist yet (pre-migration)
        let table_exists: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='consent_records'",
            [],
            |row| row.get(0),
        )?;

        if !table_exists {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            "SELECT id, consent_type, granted, timestamp FROM consent_records ORDER BY timestamp",
        )?;

        let records = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .map(|(id, ct_str, granted, ts)| (id, ct_str, granted != 0, ts as u64))
            .collect();

        Ok(records)
    }

    /// Logs an audit event.
    pub fn log_audit_event(
        &self,
        event_type: &str,
        details: Option<&str>,
    ) -> Result<(), StorageError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.conn.execute(
            "INSERT INTO audit_log (event_type, details, timestamp) VALUES (?1, ?2, ?3)",
            params![event_type, details, now as i64],
        )?;
        Ok(())
    }
}
