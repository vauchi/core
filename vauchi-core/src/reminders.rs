// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reminder / nudge state (backup reminders, own-card repropagation).
//!
//! A neutral leaf module: `ux_state`-persisted nudge trackers shared by
//! `storage` and `api` without depending on either.

fn default_true() -> bool {
    true
}

/// How often to remind about backups.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReminderFrequency {
    #[default]
    Weekly,
    Monthly,
    Never,
}

impl ReminderFrequency {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::Never => "Never",
        }
    }

    pub fn from_label(s: &str) -> Self {
        match s {
            "Monthly" => Self::Monthly,
            "Never" => Self::Never,
            _ => Self::Weekly,
        }
    }

    /// Cycle to the next frequency option.
    pub fn next(self) -> Self {
        match self {
            Self::Weekly => Self::Monthly,
            Self::Monthly => Self::Never,
            Self::Never => Self::Weekly,
        }
    }

    /// Threshold in seconds, or None if disabled.
    pub fn threshold_secs(self) -> Option<u64> {
        match self {
            Self::Weekly => Some(7 * 24 * 60 * 60),
            Self::Monthly => Some(30 * 24 * 60 * 60),
            Self::Never => None,
        }
    }
}

/// Tracks backup reminder state for progressive nudges.
///
/// Persisted encrypted in the `ux_state` table.
/// Schedule: configurable via `frequency` (Weekly, Monthly, or Never).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupReminderState {
    /// Unix epoch seconds of last successful backup.
    pub last_backup_timestamp: Option<u64>,
    /// Legacy field — use `frequency` instead. Kept for backward compat.
    #[serde(default = "default_true")]
    pub reminders_enabled: bool,
    /// Reminders shown since last backup (drives schedule).
    pub reminder_count: u32,
    /// How often to remind. Defaults to Weekly.
    #[serde(default)]
    pub frequency: ReminderFrequency,
}

impl Default for BackupReminderState {
    fn default() -> Self {
        Self::new()
    }
}

impl BackupReminderState {
    pub fn new() -> Self {
        Self {
            last_backup_timestamp: None,
            reminders_enabled: true,
            reminder_count: 0,
            frequency: ReminderFrequency::Weekly,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Record that a backup completed successfully. `now` is the
    /// Unix-epoch timestamp stamped into `last_backup_timestamp`;
    /// production callers pass `vauchi.clock().unix_seconds()`.
    pub fn record_backup(&mut self, now: u64) {
        self.last_backup_timestamp = Some(now);
        self.reminder_count = 0;
    }

    /// Record that a reminder was shown (dismissed or acted on).
    pub fn record_reminder_shown(&mut self) {
        self.reminder_count += 1;
    }

    /// Check if a reminder is due.
    /// `fallback_timestamp` is identity creation time (used when no backup exists).
    pub fn is_reminder_due(&self, now: u64, fallback_timestamp: u64) -> bool {
        let threshold = match self.frequency.threshold_secs() {
            Some(t) => t,
            None => return false,
        };
        if !self.reminders_enabled {
            return false;
        }
        let reference = self.last_backup_timestamp.unwrap_or(fallback_timestamp);
        now.saturating_sub(reference) >= threshold
    }
}

/// Durable marker: the own card changed and contacts owe a (re)propagation.
///
/// Set on an own-card edit; the sync loop runs a group-aware repropagation
/// pass and clears it only once every contact's update has been queued.
/// Decoupled from device-sync `SyncItem::CardUpdated` (contact propagation is
/// not device sync). `failed_attempts` caps consecutive failed passes so a
/// permanent error backs off instead of hot-looping; a fresh edit resets it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnCardRepropagateState {
    /// A repropagation pass is owed.
    #[serde(default)]
    pub needs_repropagate: bool,
    /// Consecutive sync passes that could not queue repropagation for every
    /// contact. Reset to 0 on a fully successful pass or on a fresh edit.
    #[serde(default)]
    pub failed_attempts: u32,
}

impl OwnCardRepropagateState {
    /// Maximum consecutive failed passes before the marker backs off (stops
    /// auto-retrying until the next edit). Bounds work on a permanent error.
    pub const MAX_FAILED_ATTEMPTS: u32 = 5;

    /// The marker owes a pass and has not exhausted its retry budget.
    pub fn should_run(&self) -> bool {
        self.needs_repropagate && self.failed_attempts < Self::MAX_FAILED_ATTEMPTS
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
