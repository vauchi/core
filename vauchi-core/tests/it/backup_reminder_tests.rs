// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for backup reminder state.

use vauchi_core::Vauchi;
use vauchi_core::types::{BackupReminderState, ReminderFrequency};

const SECS_PER_DAY: u64 = 24 * 60 * 60;
const BACKUP_PASSWORD: &str = "correct-horse-battery-staple";

// ── Serde ────────────────────────────────────────────────────────────────────

// @internal
#[test]
fn backup_reminder_state_serde_roundtrip() {
    let state = BackupReminderState::new();
    assert!(state.reminders_enabled);
    assert_eq!(state.reminder_count, 0);
    assert_eq!(state.last_backup_timestamp, None);
    assert_eq!(state.frequency, ReminderFrequency::Weekly);

    let json = state.to_json().unwrap();
    let restored = BackupReminderState::from_json(&json).unwrap();

    assert_eq!(restored.reminders_enabled, state.reminders_enabled);
    assert_eq!(restored.reminder_count, state.reminder_count);
    assert_eq!(restored.last_backup_timestamp, state.last_backup_timestamp);
    assert_eq!(restored.frequency, state.frequency);
}

// @internal
#[test]
fn backup_reminder_state_serde_backward_compat() {
    let json = r#"{"last_backup_timestamp":null,"reminders_enabled":true,"reminder_count":0}"#;
    let state = BackupReminderState::from_json(json).unwrap();
    assert_eq!(state.frequency, ReminderFrequency::Weekly);
    assert!(state.reminders_enabled);
}

// ── State mutations ──────────────────────────────────────────────────────────

// @internal
#[test]
fn backup_reminder_state_record_backup_resets() {
    let mut state = BackupReminderState::new();
    state.reminder_count = 3;

    state.record_backup(1_700_000_000);

    assert_eq!(state.last_backup_timestamp, Some(1_700_000_000));
    assert_eq!(state.reminder_count, 0);
}

// @internal
#[test]
fn backup_reminder_state_record_reminder_increments() {
    let mut state = BackupReminderState::new();
    assert_eq!(state.reminder_count, 0);

    state.record_reminder_shown();
    assert_eq!(state.reminder_count, 1);

    state.record_reminder_shown();
    assert_eq!(state.reminder_count, 2);
}

// ── Reminder scheduling logic ────────────────────────────────────────────────

// @scenario: backup_reminder :: reminders disabled
// @internal
#[test]
fn is_reminder_due_false_when_disabled() {
    let mut state = BackupReminderState::new();
    state.reminders_enabled = false;

    let identity_created = 1_000_000;
    let now = identity_created + 30 * SECS_PER_DAY; // well past any threshold
    assert!(!state.is_reminder_due(now, identity_created));

    // Also test frequency=Never disables reminders
    let mut state2 = BackupReminderState::new();
    state2.frequency = ReminderFrequency::Never;
    assert!(!state2.is_reminder_due(now, identity_created));
}

// @scenario: backup_reminder :: first reminder after 7 days
// @internal
#[test]
fn is_reminder_due_after_7_days_first_time() {
    let state = BackupReminderState::new(); // no backup, reminder_count=0
    let identity_created = 1_000_000;
    let now = identity_created + 8 * SECS_PER_DAY;
    assert!(state.is_reminder_due(now, identity_created));
}

// @scenario: backup_reminder :: No reminder before the weekly threshold
// @internal
#[test]
fn is_reminder_not_due_before_7_days() {
    let state = BackupReminderState::new();
    let identity_created = 1_000_000;
    let now = identity_created + 5 * SECS_PER_DAY;
    assert!(!state.is_reminder_due(now, identity_created));
}

// @scenario: backup_reminder :: monthly frequency at 30-day threshold
// @internal
#[test]
fn is_reminder_due_after_30_days_monthly() {
    let mut state = BackupReminderState::new();
    state.frequency = ReminderFrequency::Monthly;
    let last_backup = 1_000_000;
    state.last_backup_timestamp = Some(last_backup);

    let now = last_backup + 31 * SECS_PER_DAY;
    assert!(state.is_reminder_due(now, 0));

    // Not due at 29 days
    let now_early = last_backup + 29 * SECS_PER_DAY;
    assert!(!state.is_reminder_due(now_early, 0));
}

// @scenario: backup_reminder :: monthly frequency triggers at 30 days
// @internal
#[test]
fn is_reminder_due_monthly_frequency() {
    let mut state = BackupReminderState::new();
    state.frequency = ReminderFrequency::Monthly;
    let identity_created = 1_000_000;

    // Not due at 7 days (weekly threshold)
    let now_7d = identity_created + 8 * SECS_PER_DAY;
    assert!(!state.is_reminder_due(now_7d, identity_created));

    // Due at 31 days (monthly threshold)
    let now_31d = identity_created + 31 * SECS_PER_DAY;
    assert!(state.is_reminder_due(now_31d, identity_created));
}

// @scenario: backup_reminder :: frequency cycling
// @internal
#[test]
fn reminder_frequency_cycles() {
    assert_eq!(ReminderFrequency::Weekly.next(), ReminderFrequency::Monthly);
    assert_eq!(ReminderFrequency::Monthly.next(), ReminderFrequency::Never);
    assert_eq!(ReminderFrequency::Never.next(), ReminderFrequency::Weekly);
}

// @internal
#[test]
fn reminder_frequency_label_roundtrip() {
    for freq in [
        ReminderFrequency::Weekly,
        ReminderFrequency::Monthly,
        ReminderFrequency::Never,
    ] {
        assert_eq!(ReminderFrequency::from_label(freq.label()), freq);
    }
    // Unknown label defaults to Weekly
    assert_eq!(
        ReminderFrequency::from_label("bogus"),
        ReminderFrequency::Weekly
    );
}

// ── Storage persistence ──────────────────────────────────────────────────────

// @internal
#[test]
fn backup_reminder_storage_roundtrip() {
    let vauchi = Vauchi::in_memory().unwrap();

    let loaded = vauchi.load_backup_reminder_state().unwrap();
    assert!(loaded.reminders_enabled);
    assert_eq!(loaded.reminder_count, 0);
    assert_eq!(loaded.last_backup_timestamp, None);

    let mut state = BackupReminderState::new();
    state.reminders_enabled = false;
    state.reminder_count = 5;
    state.last_backup_timestamp = Some(1_700_000_000);
    vauchi.save_backup_reminder_state(&state).unwrap();

    let restored = vauchi.load_backup_reminder_state().unwrap();
    assert!(!restored.reminders_enabled);
    assert_eq!(restored.reminder_count, 5);
    assert_eq!(restored.last_backup_timestamp, Some(1_700_000_000));
}

// ── Export integration ───────────────────────────────────────────────────────

// @scenario: backup_reminder :: export updates reminder state
// @internal
#[test]
fn export_backup_updates_reminder_state() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("TestUser").unwrap();

    // Before export, no backup timestamp
    let state_before = vauchi.load_backup_reminder_state().unwrap();
    assert_eq!(state_before.last_backup_timestamp, None);

    let _backup = vauchi.export_backup(BACKUP_PASSWORD).unwrap();

    let state_after = vauchi.load_backup_reminder_state().unwrap();
    assert!(
        state_after.last_backup_timestamp.is_some(),
        "export_backup should set last_backup_timestamp"
    );
    assert_eq!(state_after.reminder_count, 0);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let ts = state_after.last_backup_timestamp.unwrap();
    assert!(now - ts < 5, "timestamp should be recent");
}
