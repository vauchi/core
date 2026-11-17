// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The loss-prevention prompt (vauchi/private#796 plan step 9, D8a to D8d).
//! While the user has no linked device, no guardians and no backup from the
//! last 30 days, the backup reminder's slot carries a prompt naming all
//! three; any one safeguard brings the ordinary reminder back.

use vauchi_app::ui::{ActionResult, AppEngine, UserAction, WorkflowEngine};
use vauchi_core::Vauchi;
use vauchi_core::types::{BackupReminderState, ReminderFrequency};
use vauchi_core_testkit::device_sync::link_tablet;
use vauchi_core_testkit::helpers::create_vauchi_with_identity;
use vauchi_core_testkit::two_recipient::add_recipient;

const DAY: u64 = 24 * 60 * 60;
const LAPTOP_SEED: [u8; 32] = [9u8; 32];
const PROMPT: &str = "If you lose this phone, your contact list goes with it. \
                      Link a second device, choose guardians or make a backup to keep it.";

/// A due reminder. `None` stands for "never backed up": an in-memory
/// identity is created now, so a backup at the epoch makes the reminder due
/// the way an identity created long ago would.
fn alice_due(frequency: ReminderFrequency, backed_up_days_ago: Option<u64>) -> Vauchi {
    let vauchi = create_vauchi_with_identity("Alice");
    let now = vauchi.clock().unix_seconds();
    let mut state = BackupReminderState::new();
    state.frequency = frequency;
    state.last_backup_timestamp = Some(backed_up_days_ago.map_or(1, |days| now - days * DAY));
    vauchi.save_backup_reminder_state(&state).unwrap();
    vauchi
}

fn designate_a_guardian(vauchi: &Vauchi) {
    let alice_pk = *vauchi.identity().unwrap().signing_public_key();
    let bob = add_recipient(vauchi, &alice_pk, "Bob");
    vauchi
        .verify_contact_fingerprint(&bob.id_at_sharer)
        .unwrap();
    vauchi
        .trust_for_recovery_in_group(&bob.id_at_sharer, None)
        .unwrap();
}

fn toast_action(reminder: Option<ActionResult>) -> Option<String> {
    match reminder {
        Some(ActionResult::ShowToast { undo_action_id, .. }) => undo_action_id,
        None => None,
        other => panic!("a reminder is a toast or nothing, got {other:?}"),
    }
}

// @scenario: backup_reminder :: A user with no safeguard is told how to avoid losing their contacts
#[test]
fn with_no_safeguard_the_reminder_slot_offers_all_three_and_opens_settings() {
    let mut engine = AppEngine::new(alice_due(ReminderFrequency::Weekly, None));

    assert_eq!(
        engine.drain_backup_reminder(),
        Some(ActionResult::ShowToast {
            message: PROMPT.into(),
            undo_action_id: Some("protect_now".into()),
            undo_label: Some("Set up".into()),
        })
    );
    match engine.handle_action(UserAction::UndoPressed {
        action_id: "protect_now".into(),
    }) {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "settings"),
        other => panic!("expected Settings navigation, got {other:?}"),
    }
}

// @scenario: backup_reminder :: One safeguard is enough to remove the safeguard prompt
#[test]
fn a_linked_device_brings_back_the_backup_reminder() {
    let vauchi = alice_due(ReminderFrequency::Weekly, None);
    link_tablet(&vauchi, LAPTOP_SEED);

    let action = toast_action(AppEngine::new(vauchi).drain_backup_reminder());

    assert_eq!(action.as_deref(), Some("backup_now"));
}

// @scenario: backup_reminder :: One safeguard is enough to remove the safeguard prompt
#[test]
fn a_designated_guardian_brings_back_the_backup_reminder() {
    let vauchi = alice_due(ReminderFrequency::Weekly, None);
    designate_a_guardian(&vauchi);

    let action = toast_action(AppEngine::new(vauchi).drain_backup_reminder());

    assert_eq!(action.as_deref(), Some("backup_now"));
}

// @scenario: backup_reminder :: One safeguard is enough to remove the safeguard prompt
#[test]
fn a_backup_from_ten_days_ago_brings_back_the_backup_reminder() {
    let vauchi = alice_due(ReminderFrequency::Weekly, Some(10));

    let action = toast_action(AppEngine::new(vauchi).drain_backup_reminder());

    assert_eq!(action.as_deref(), Some("backup_now"));
}

// @scenario: backup_reminder :: A backup older than 30 days no longer counts as a safeguard
#[test]
fn a_backup_from_thirty_one_days_ago_is_not_a_safeguard() {
    let vauchi = alice_due(ReminderFrequency::Weekly, Some(31));

    let action = toast_action(AppEngine::new(vauchi).drain_backup_reminder());

    assert_eq!(action.as_deref(), Some("protect_now"));
}

// @scenario: backup_reminder :: Reminders set to "Never" also silence the safeguard prompt
#[test]
fn reminders_set_to_never_show_no_safeguard_prompt() {
    let vauchi = alice_due(ReminderFrequency::Never, None);

    assert_eq!(AppEngine::new(vauchi).drain_backup_reminder(), None);
}

// @internal — showing the prompt counts against the reminder schedule like
// the backup toast it replaces (D8b: same cadence).
#[test]
fn showing_the_safeguard_prompt_counts_as_a_reminder_shown() {
    let mut engine = AppEngine::new(alice_due(ReminderFrequency::Weekly, None));

    let action = toast_action(engine.drain_backup_reminder());

    assert_eq!(action.as_deref(), Some("protect_now"));
    let state = engine.vauchi().load_backup_reminder_state().unwrap();
    assert_eq!(state.reminder_count, 1);
}
