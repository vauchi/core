// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`UxStore`](super::UxStore).

use super::{Storage, StorageError};
use crate::types::AhaMomentTracker;
use crate::types::BackupReminderState;
use crate::types::DemoContactState;
use crate::types::OnboardingProgress;
use crate::types::SettingsFlags;

impl Storage {
    /// Saves the aha moments tracker state (encrypted).
    pub fn save_aha_tracker(&self, tracker: &AhaMomentTracker) -> Result<(), StorageError> {
        self.ux().save_aha_tracker(tracker)
    }
    /// Loads the aha moments tracker state (decrypted).
    pub fn load_aha_tracker(&self) -> Result<Option<AhaMomentTracker>, StorageError> {
        self.ux().load_aha_tracker()
    }
    /// Loads aha tracker or creates a new one if none exists.
    pub fn load_or_create_aha_tracker(&self) -> Result<AhaMomentTracker, StorageError> {
        self.ux().load_or_create_aha_tracker()
    }
    /// Saves the demo contact state (encrypted).
    pub fn save_demo_contact_state(&self, state: &DemoContactState) -> Result<(), StorageError> {
        self.ux().save_demo_contact_state(state)
    }
    /// Loads the demo contact state (decrypted).
    pub fn load_demo_contact_state(&self) -> Result<Option<DemoContactState>, StorageError> {
        self.ux().load_demo_contact_state()
    }
    /// Loads demo contact state or creates a new active one if
    /// none exists. `now` is the Unix-epoch timestamp stamped into
    /// a fresh state's `last_update_timestamp` via the
    /// `Storage`-owned [`Clock`](crate::clock::Clock).
    pub fn load_or_create_demo_contact_state(&self) -> Result<DemoContactState, StorageError> {
        self.ux().load_or_create_demo_contact_state()
    }
    /// Checks if the demo contact is currently active.
    pub fn is_demo_contact_active(&self) -> Result<bool, StorageError> {
        self.ux().is_demo_contact_active()
    }
    /// Saves the onboarding progress (encrypted).
    pub fn save_onboarding_progress(
        &self,
        progress: &OnboardingProgress,
    ) -> Result<(), StorageError> {
        self.ux().save_onboarding_progress(progress)
    }
    /// Loads the onboarding progress (decrypted).
    pub fn load_onboarding_progress(&self) -> Result<Option<OnboardingProgress>, StorageError> {
        self.ux().load_onboarding_progress()
    }
    /// Loads onboarding progress or creates a new one if none exists.
    ///
    /// `now` is the Unix-epoch timestamp to stamp into a freshly-
    /// created progress's `started_at`. Storage does not yet hold a
    /// [`Clock`](crate::clock::Clock) — the `Storage`-owned
    /// clock stamps `started_at` for freshly-created records.
    pub fn load_or_create_onboarding_progress(&self) -> Result<OnboardingProgress, StorageError> {
        self.ux().load_or_create_onboarding_progress()
    }
    /// Saves the backup reminder state (encrypted).
    pub fn save_backup_reminder_state(
        &self,
        state: &BackupReminderState,
    ) -> Result<(), StorageError> {
        self.ux().save_backup_reminder_state(state)
    }
    /// Loads the backup reminder state (decrypted).
    pub fn load_backup_reminder_state(&self) -> Result<Option<BackupReminderState>, StorageError> {
        self.ux().load_backup_reminder_state()
    }
    /// Saves the settings flags (encrypted).
    pub fn save_settings_flags(&self, flags: &SettingsFlags) -> Result<(), StorageError> {
        self.ux().save_settings_flags(flags)
    }
    /// Loads the settings flags (decrypted).
    pub fn load_settings_flags(&self) -> Result<Option<SettingsFlags>, StorageError> {
        self.ux().load_settings_flags()
    }
    /// Saves both aha tracker and demo contact state atomically (encrypted).
    pub fn save_ux_state(
        &self,
        aha_tracker: &AhaMomentTracker,
        demo_state: &DemoContactState,
    ) -> Result<(), StorageError> {
        self.ux().save_ux_state(aha_tracker, demo_state)
    }
    /// Loads both aha tracker and demo contact state. `now` is
    /// Loads both aha tracker and demo contact state. Freshly-
    /// created records take their `last_update_timestamp` from
    /// the `Storage`-owned [`Clock`](crate::clock::Clock).
    pub fn load_ux_state(&self) -> Result<(AhaMomentTracker, DemoContactState), StorageError> {
        self.ux().load_ux_state()
    }
}
