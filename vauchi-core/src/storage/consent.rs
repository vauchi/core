// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`ConsentStore`](super::ConsentStore).

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
        self.consent()
            .execute_consent_upsert(id, consent_type, granted, timestamp)
    }
    /// Checks if consent is granted for a type (latest record).
    pub fn check_consent(&self, consent_type: &str) -> Result<bool, StorageError> {
        self.consent().check_consent(consent_type)
    }
    /// Lists all consent records as tuples of (id, consent_type, granted, timestamp).
    ///
    /// Returns raw tuples to avoid circular dependency with the api::consent module.
    pub fn list_consent_records(&self) -> Result<Vec<(String, String, bool, u64)>, StorageError> {
        self.consent().list_consent_records()
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
        self.consent().execute_consent_upsert_with_version(
            id,
            consent_type,
            granted,
            timestamp,
            policy_version,
        )
    }
    /// Lists all consent records including policy version.
    ///
    /// Returns tuples of (id, consent_type, granted, timestamp, policy_version).
    #[allow(clippy::type_complexity)]
    pub fn list_consent_records_with_version(
        &self,
    ) -> Result<Vec<(String, String, bool, u64, Option<String>)>, StorageError> {
        self.consent().list_consent_records_with_version()
    }
    /// Saves the deletion state (encrypted).
    pub fn save_deletion_state(&self, state: &super::DeletionState) -> Result<(), StorageError> {
        self.consent().save_deletion_state(state)
    }
    /// Loads the deletion state (decrypted).
    pub fn load_deletion_state(&self) -> Result<super::DeletionState, StorageError> {
        self.consent().load_deletion_state()
    }
    /// Lists all audit log entries, decrypting details where applicable.
    ///
    /// Returns tuples of (event_type, details, timestamp).
    /// Encrypted details are decrypted with the storage key; falls back to
    /// plaintext `details` column for pre-encryption entries.
    pub fn list_audit_log(&self) -> Result<Vec<(String, Option<String>, u64)>, StorageError> {
        self.consent().list_audit_log()
    }
    /// Logs an audit event (details encrypted if present).
    pub fn log_audit_event(
        &self,
        event_type: &str,
        details: Option<&str>,
    ) -> Result<(), StorageError> {
        self.consent().log_audit_event(event_type, details)
    }
}
