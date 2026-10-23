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
use crate::onboarding::AhaMomentTracker;
use crate::onboarding::DemoContactState;
use crate::onboarding::OnboardingProgress;
use crate::reminders::BackupReminderState;
use crate::settings::SettingsFlags;

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
            "INSERT OR REPLACE INTO ux_state (id, aha_tracker_json_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET aha_tracker_json_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Loads the aha moments tracker state (decrypted).
    pub fn load_aha_tracker(&self) -> Result<Option<AhaMomentTracker>, StorageError> {
        let result = self.conn.query_row(
            "SELECT aha_tracker_json_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );

        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
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
            "INSERT OR REPLACE INTO ux_state (id, demo_contact_json_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET demo_contact_json_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Loads the demo contact state (decrypted).
    pub fn load_demo_contact_state(&self) -> Result<Option<DemoContactState>, StorageError> {
        let result = self.conn.query_row(
            "SELECT demo_contact_json_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );

        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
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
    /// Saves the last-used exchange defaults (encrypted). M2 S1
    /// (`2026-07-03-one-tap-exchange`): written on every mode commit so a
    /// repeat exchange skips the group gate with the prior selection.
    pub fn save_exchange_defaults(
        &self,
        defaults: &crate::exchange::ExchangeDefaults,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(defaults)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(self.key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let now = self.now_secs();
        self.conn.execute(
            "INSERT INTO ux_state (id, exchange_defaults_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET exchange_defaults_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;
        Ok(())
    }
    /// Loads the last-used exchange defaults, if any were stored.
    pub fn load_exchange_defaults(
        &self,
    ) -> Result<Option<crate::exchange::ExchangeDefaults>, StorageError> {
        let result = self.conn.query_row(
            "SELECT exchange_defaults_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );
        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let defaults = serde_json::from_slice(&decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(defaults))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
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
    /// Saves the own-card repropagation marker (encrypted).
    pub fn save_own_card_repropagate(
        &self,
        state: &crate::reminders::OwnCardRepropagateState,
    ) -> Result<(), StorageError> {
        let json = state
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let encrypted = crate::crypto::encrypt(self.key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, own_card_repropagate_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET own_card_repropagate_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }

    /// Loads the own-card repropagation marker, defaulting to "not owed".
    pub fn load_own_card_repropagate(
        &self,
    ) -> Result<crate::reminders::OwnCardRepropagateState, StorageError> {
        let result = self.conn.query_row(
            "SELECT own_card_repropagate_encrypted FROM ux_state WHERE id = 1",
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
                let state = crate::reminders::OwnCardRepropagateState::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(state)
            }
            Ok(_) => Ok(crate::reminders::OwnCardRepropagateState::default()),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Ok(crate::reminders::OwnCardRepropagateState::default())
            }
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Saves the persisted relay URL with its OHTTP trust anchor (encrypted).
    /// Both are written together, so a relay never keeps another's anchor.
    pub fn save_relay(
        &self,
        url: &str,
        ohttp_anchor: Option<&[u8; 32]>,
    ) -> Result<(), StorageError> {
        let encrypt = |plain: &[u8]| {
            crate::crypto::encrypt(self.key, plain)
                .map_err(|e| StorageError::Encryption(e.to_string()))
        };
        let url_encrypted = encrypt(url.as_bytes())?;
        let anchor_encrypted = ohttp_anchor.map(|anchor| encrypt(anchor)).transpose()?;
        let now = self.now_secs();
        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, relay_url_encrypted, relay_anchor_encrypted, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET relay_url_encrypted = ?1, relay_anchor_encrypted = ?2, updated_at = ?3",
            params![url_encrypted, anchor_encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Saves the digest of this device's user-presence secret (encrypted).
    pub fn save_user_presence_digest(&self, digest: &[u8; 32]) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, digest)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let now = self.now_secs();
        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, user_presence_digest_encrypted, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET user_presence_digest_encrypted = ?1, updated_at = ?2",
            params![encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Removes the digest of this device's user-presence secret.
    pub fn clear_user_presence_digest(&self) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE ux_state SET user_presence_digest_encrypted = NULL, updated_at = ?1 WHERE id = 1",
            params![self.now_secs() as i64],
        )?;
        Ok(())
    }

    /// Loads the digest of this device's user-presence secret, if enrolled.
    pub fn load_user_presence_digest(&self) -> Result<Option<[u8; 32]>, StorageError> {
        let result = self.conn.query_row(
            "SELECT user_presence_digest_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );
        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let digest = <[u8; 32]>::try_from(decrypted.as_slice()).map_err(|_| {
                    StorageError::Serialization("user-presence digest is not 32 bytes".into())
                })?;
                Ok(Some(digest))
            }
            Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Loads the persisted relay's OHTTP trust anchor (decrypted), if any.
    pub fn load_relay_anchor(&self) -> Result<Option<[u8; 32]>, StorageError> {
        let result = self.conn.query_row(
            "SELECT relay_anchor_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );
        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let anchor = <[u8; 32]>::try_from(decrypted.as_slice()).map_err(|_| {
                    StorageError::Serialization("relay anchor is not 32 bytes".into())
                })?;
                Ok(Some(anchor))
            }
            Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
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
            "INSERT INTO ux_state (id, aha_tracker_json_encrypted, demo_contact_json_encrypted, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
               aha_tracker_json_encrypted = ?1,
               demo_contact_json_encrypted = ?2,
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
