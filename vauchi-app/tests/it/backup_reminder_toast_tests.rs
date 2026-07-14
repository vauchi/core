// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Actionable backup-reminder toast contract.

use vauchi_app::ui::{ActionResult, AppEngine, UserAction, WorkflowEngine};
use vauchi_core::Vauchi;
use vauchi_core::types::{BackupReminderState, ReminderFrequency};

fn due_engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let mut state = BackupReminderState::new();
    state.frequency = ReminderFrequency::Weekly;
    state.last_backup_timestamp = Some(1);
    vauchi.save_backup_reminder_state(&state).unwrap();

    AppEngine::new(vauchi)
}

// @scenario: backup-reminder :: native toast action opens Backup
// @internal
#[test]
fn backup_reminder_carries_core_label_and_accepts_toast_action() {
    let mut engine = due_engine();

    let reminder = engine.drain_backup_reminder().expect("reminder is due");
    assert_eq!(
        reminder,
        ActionResult::ShowToast {
            message: "You haven't backed up in a while. Back up now to protect your identity."
                .into(),
            undo_action_id: Some("backup_now".into()),
            undo_label: Some("Create Backup".into()),
        }
    );

    let result = engine.handle_action(UserAction::UndoPressed {
        action_id: "backup_now".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "backup"),
        other => panic!("expected Backup navigation, got {other:?}"),
    }
}
