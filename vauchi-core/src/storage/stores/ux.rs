// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ux domain persistence view (ux_state).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use rusqlite::{Connection, params};
use std::sync::Arc;

use super::super::{Storage, StorageError};
use crate::types::AhaMomentTracker;
use crate::types::BackupReminderState;
use crate::types::DemoContactState;
use crate::types::OnboardingProgress;
use crate::types::SettingsFlags;

/// Scoped persistence view for the ux domain.
pub struct UxStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the ux domain.
    pub fn ux(&self) -> UxStore<'_> {
        UxStore {
            conn: &self.conn,
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

impl UxStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
    /// Saves the aha moments tracker state (encrypted).
    pub fn save_aha_tracker(&self, tracker: &AhaMomentTracker) -> Result<(), StorageError> {
        let json = tracker
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let encrypted = crate::crypto::encrypt(self.key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, aha_tracker_json, aha_tracker_json_encrypted, updated_at)
             VALUES (1, '', ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET aha_tracker_json = '', aha_tracker_json_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Loads the aha moments tracker state (decrypted).
    pub fn load_aha_tracker(&self) -> Result<Option<AhaMomentTracker>, StorageError> {
        let result = self.conn.query_row(
            "SELECT aha_tracker_json_encrypted, aha_tracker_json FROM ux_state WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: Option<String> = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let tracker = AhaMomentTracker::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(tracker))
            }
            Ok((_, Some(json))) if !json.is_empty() => {
                let tracker = AhaMomentTracker::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(tracker))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Loads aha tracker or creates a new one if none exists.
    pub fn load_or_create_aha_tracker(&self) -> Result<AhaMomentTracker, StorageError> {
        match self.load_aha_tracker()? {
            Some(tracker) => Ok(tracker),
            None => Ok(AhaMomentTracker::new()),
        }
    }
    /// Saves the demo contact state (encrypted).
    pub fn save_demo_contact_state(&self, state: &DemoContactState) -> Result<(), StorageError> {
        let json = state
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let encrypted = crate::crypto::encrypt(self.key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, demo_contact_json, demo_contact_json_encrypted, updated_at)
             VALUES (1, '', ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET demo_contact_json = '', demo_contact_json_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Loads the demo contact state (decrypted).
    pub fn load_demo_contact_state(&self) -> Result<Option<DemoContactState>, StorageError> {
        let result = self.conn.query_row(
            "SELECT demo_contact_json_encrypted, demo_contact_json FROM ux_state WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: Option<String> = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let state = DemoContactState::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Ok((_, Some(json))) if !json.is_empty() => {
                let state = DemoContactState::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Loads demo contact state or creates a new active one if
    /// none exists. `now` is the Unix-epoch timestamp stamped into
    /// a fresh state's `last_update_timestamp` via the
    /// `Storage`-owned [`Clock`](crate::clock::Clock).
    pub fn load_or_create_demo_contact_state(&self) -> Result<DemoContactState, StorageError> {
        match self.load_demo_contact_state()? {
            Some(state) => Ok(state),
            None => Ok(DemoContactState::new_active(self.now_secs())),
        }
    }
    /// Checks if the demo contact is currently active.
    pub fn is_demo_contact_active(&self) -> Result<bool, StorageError> {
        match self.load_demo_contact_state()? {
            Some(state) => Ok(state.is_active),
            None => Ok(false), // Not yet initialized
        }
    }
    /// Saves the onboarding progress (encrypted).
    pub fn save_onboarding_progress(
        &self,
        progress: &OnboardingProgress,
    ) -> Result<(), StorageError> {
        let json = progress
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let encrypted = crate::crypto::encrypt(self.key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, onboarding_progress_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET onboarding_progress_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Loads the onboarding progress (decrypted).
    pub fn load_onboarding_progress(&self) -> Result<Option<OnboardingProgress>, StorageError> {
        let result = self.conn.query_row(
            "SELECT onboarding_progress_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                Ok(encrypted)
            },
        );

        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let progress = OnboardingProgress::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(progress))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Loads onboarding progress or creates a new one if none exists.
    ///
    /// `now` is the Unix-epoch timestamp to stamp into a freshly-
    /// created progress's `started_at`. Storage does not yet hold a
    /// [`Clock`](crate::clock::Clock) — the `Storage`-owned
    /// clock stamps `started_at` for freshly-created records.
    pub fn load_or_create_onboarding_progress(&self) -> Result<OnboardingProgress, StorageError> {
        match self.load_onboarding_progress()? {
            Some(progress) => Ok(progress),
            None => Ok(OnboardingProgress::new(self.now_secs())),
        }
    }
    /// Saves the backup reminder state (encrypted).
    pub fn save_backup_reminder_state(
        &self,
        state: &BackupReminderState,
    ) -> Result<(), StorageError> {
        let json = state
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let encrypted = crate::crypto::encrypt(self.key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, backup_reminder_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET backup_reminder_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Loads the backup reminder state (decrypted).
    pub fn load_backup_reminder_state(&self) -> Result<Option<BackupReminderState>, StorageError> {
        let result = self.conn.query_row(
            "SELECT backup_reminder_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                Ok(encrypted)
            },
        );

        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let state = BackupReminderState::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Saves the settings flags (encrypted).
    pub fn save_settings_flags(&self, flags: &SettingsFlags) -> Result<(), StorageError> {
        let json = flags
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let encrypted = crate::crypto::encrypt(self.key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, settings_flags_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET settings_flags_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Saves the persisted relay URL (encrypted).
    pub fn save_relay_url(&self, url: &str) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, url.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let now = self.now_secs();
        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, relay_url_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET relay_url_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Loads the persisted relay URL (decrypted), if any.
    pub fn load_relay_url(&self) -> Result<Option<String>, StorageError> {
        let result = self.conn.query_row(
            "SELECT relay_url_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                Ok(encrypted)
            },
        );

        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let url = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(url))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Loads the settings flags (decrypted).
    pub fn load_settings_flags(&self) -> Result<Option<SettingsFlags>, StorageError> {
        let result = self.conn.query_row(
            "SELECT settings_flags_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                Ok(encrypted)
            },
        );

        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let flags = SettingsFlags::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(flags))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Saves both aha tracker and demo contact state atomically (encrypted).
    pub fn save_ux_state(
        &self,
        aha_tracker: &AhaMomentTracker,
        demo_state: &DemoContactState,
    ) -> Result<(), StorageError> {
        let aha_json = aha_tracker
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let demo_json = demo_state
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let aha_encrypted = crate::crypto::encrypt(self.key, aha_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let demo_encrypted = crate::crypto::encrypt(self.key, demo_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        // Use INSERT ... ON CONFLICT UPDATE to preserve other columns
        // (e.g. onboarding_progress_encrypted) that are not part of this save.
        self.conn.execute(
            "INSERT INTO ux_state (id, aha_tracker_json, aha_tracker_json_encrypted, demo_contact_json, demo_contact_json_encrypted, updated_at)
             VALUES (1, '', ?1, '', ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
               aha_tracker_json = '', aha_tracker_json_encrypted = ?1,
               demo_contact_json = '', demo_contact_json_encrypted = ?2,
               updated_at = ?3",
            params![aha_encrypted, demo_encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Loads both aha tracker and demo contact state. `now` is
    /// Loads both aha tracker and demo contact state. Freshly-
    /// created records take their `last_update_timestamp` from
    /// the `Storage`-owned [`Clock`](crate::clock::Clock).
    pub fn load_ux_state(&self) -> Result<(AhaMomentTracker, DemoContactState), StorageError> {
        let aha_tracker = self.load_or_create_aha_tracker()?;
        let demo_state = self.load_or_create_demo_contact_state()?;
        Ok((aha_tracker, demo_state))
    }
}
