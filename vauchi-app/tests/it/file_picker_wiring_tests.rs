// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the ADR-031 file-picker wiring.
//!
//! Phase 2A of `2026-05-03-core-file-picker-command`: when the user
//! triggers `import_contacts` on the More screen, AppEngine emits an
//! `Command::FilePickFromUser`. The frontend opens the native
//! picker, then sends `FilePickedFromUser{bytes, filename}` back. Core
//! routes the bytes by current screen and calls
//! `Vauchi::import_contacts_from_vcf`, returning a toast with the
//! imported / skipped counts.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event, FilePickPurpose};

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

const VALID_VCF: &[u8] = b"\
BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:Bob Smith\r\n\
N:Smith;Bob;;;\r\n\
EMAIL:bob@example.com\r\n\
END:VCARD\r\n\
BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:Carol Jones\r\n\
N:Jones;Carol;;;\r\n\
END:VCARD\r\n";

// @scenario: core_file_picker :: import_contacts action emits FilePickFromUser
#[test]
fn import_contacts_action_emits_file_pick_command() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::More);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "import_contacts".into(),
    });
    match result {
        ActionResult::Commands { commands } => {
            assert_eq!(
                commands.len(),
                1,
                "expected one command, got {:?}",
                commands
            );
            match &commands[0] {
                Command::FilePickFromUser {
                    accepted_mime_types,
                    purpose,
                } => {
                    assert_eq!(*purpose, FilePickPurpose::ImportContacts);
                    assert!(
                        accepted_mime_types.iter().any(|m| m == "text/vcard"),
                        "expected text/vcard in accepted MIME types, got {:?}",
                        accepted_mime_types
                    );
                }
                other => panic!("expected FilePickFromUser, got {:?}", other),
            }
        }
        other => panic!("expected Commands, got {:?}", other),
    }
}

// @scenario: core_file_picker :: FilePickedFromUser on More imports vCard
#[test]
fn file_picked_from_more_imports_vcards() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::More);

    let result = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: VALID_VCF.to_vec(),
        filename: "contacts.vcf".into(),
    });

    match result {
        Some(ActionResult::ShowToast { message, .. }) => {
            assert!(
                message.contains("Imported 2"),
                "expected 'Imported 2' in toast, got {:?}",
                message
            );
        }
        other => panic!("expected ShowToast for successful import, got {:?}", other),
    }
}

// @scenario: core_file_picker :: imported contacts persisted in storage
#[test]
fn file_picked_from_more_persists_contacts() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::More);

    let _ = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: VALID_VCF.to_vec(),
        filename: "contacts.vcf".into(),
    });

    let count = engine.vauchi().list_contacts().unwrap().len();
    assert_eq!(count, 2, "expected 2 contacts after import, got {}", count);
}

// @scenario: core_file_picker :: garbage bytes import zero contacts cleanly
#[test]
fn file_picked_with_non_vcard_bytes_imports_zero_contacts() {
    // The vCard parser is intentionally lenient — it skips lines it
    // doesn't understand and returns whatever vCards it finds. Garbage
    // input produces zero entries and a clean "Imported 0 contacts"
    // toast, not an error. This is the desired UX: the user sees a
    // clear empty-result message instead of a stack trace.
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::More);

    let garbage = b"NOT A VCARD AT ALL".to_vec();
    let result = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: garbage,
        filename: "garbage.txt".into(),
    });

    match result {
        Some(ActionResult::ShowToast { message, .. }) => {
            assert!(
                message.contains("Imported 0"),
                "expected 'Imported 0' on garbage input, got {:?}",
                message
            );
        }
        other => panic!("expected ShowToast (lenient parser path), got {:?}", other),
    }

    // Storage stays clean.
    let count = engine.vauchi().list_contacts().unwrap().len();
    assert_eq!(count, 0, "no contacts should be imported from garbage");
}

// @scenario: core_file_picker :: cancellation produces no result
#[test]
fn file_pick_cancelled_returns_none() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::More);

    let result = engine.handle_hardware_event(Event::FilePickCancelledByUser);
    assert!(
        result.is_none(),
        "cancellation must produce None (no toast / alert), got {:?}",
        result
    );
}

// @scenario: core_file_picker :: bytes from non-More screen are ignored
#[test]
fn file_picked_outside_more_is_ignored() {
    let mut engine = engine_with_identity();
    // Settings is not a participating screen for Phase 2A.
    let _ = engine.navigate_to(AppScreen::Settings);

    let result = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: VALID_VCF.to_vec(),
        filename: "contacts.vcf".into(),
    });

    assert!(
        result.is_none(),
        "FilePickedFromUser on a non-participating screen must return None, got {:?}",
        result
    );

    let count = engine.vauchi().list_contacts().unwrap().len();
    assert_eq!(
        count, 0,
        "no contacts should be imported when picker fires off the wired screens"
    );
}

// @scenario: core_file_picker :: skipped duplicates surface in toast
#[test]
fn file_picked_with_duplicates_reports_skipped_count() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::More);

    // Import once — both succeed.
    let _ = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: VCF_WITH_UID.to_vec(),
        filename: "contacts.vcf".into(),
    });

    // Import the same vCards again — both should be skipped (UID match).
    let result = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: VCF_WITH_UID.to_vec(),
        filename: "contacts.vcf".into(),
    });

    match result {
        Some(ActionResult::ShowToast { message, .. }) => {
            assert!(
                message.contains("Imported 0"),
                "expected 'Imported 0' on full duplicate import, got {:?}",
                message
            );
            assert!(
                message.contains("skipped"),
                "expected 'skipped' in toast, got {:?}",
                message
            );
        }
        other => panic!("expected ShowToast, got {:?}", other),
    }
}

const VCF_WITH_UID: &[u8] = b"\
BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
UID:bob-uid-1\r\n\
FN:Bob Smith\r\n\
N:Smith;Bob;;;\r\n\
END:VCARD\r\n\
BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
UID:carol-uid-1\r\n\
FN:Carol Jones\r\n\
N:Jones;Carol;;;\r\n\
END:VCARD\r\n";

// ── Phase 2B: backup-restore via file picker ───────────────────────

fn fresh_engine() -> AppEngine {
    let vauchi = Vauchi::in_memory().unwrap();
    AppEngine::new(vauchi)
}

/// Helper: produce a real encrypted backup using a throwaway Vauchi
/// then return the hex-string bytes. Mirrors the file the user would
/// have picked via the OS picker.
fn make_backup(password: &str, name: &str) -> Vec<u8> {
    let mut donor = Vauchi::in_memory().unwrap();
    donor.create_identity(name).unwrap();
    let backup_hex = donor.export_full_backup(password).unwrap();
    backup_hex.into_bytes()
}

// @scenario: core_file_picker :: restore_backup emits FilePickFromUser
#[test]
fn restore_backup_emits_file_pick_for_backup() {
    let mut engine = fresh_engine();
    let _ = engine.navigate_to(AppScreen::Onboarding);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore_backup".into(),
    });
    match result {
        ActionResult::Commands { commands } => {
            assert_eq!(commands.len(), 1);
            match &commands[0] {
                Command::FilePickFromUser { purpose, .. } => {
                    assert_eq!(*purpose, FilePickPurpose::ImportBackup);
                }
                other => panic!("expected FilePickFromUser, got {other:?}"),
            }
        }
        other => panic!("expected Commands, got {other:?}"),
    }
}

// @scenario: core_file_picker :: file pick on Onboarding → BackupPasswordEntry
#[test]
fn file_picked_on_onboarding_transitions_to_backup_password_entry() {
    let mut engine = fresh_engine();
    let _ = engine.navigate_to(AppScreen::Onboarding);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore_backup".into(),
    });

    let result = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: make_backup("correct horse battery staple", "Alice"),
        filename: "alice-backup.txt".into(),
    });

    match result {
        Some(ActionResult::NavigateTo(screen)) => {
            assert_eq!(screen.screen_id, "backup_password_entry");
        }
        other => panic!("expected NavigateTo(backup_password_entry), got {other:?}"),
    }
}

// @scenario: core_file_picker :: submit valid password completes restore
#[test]
fn submit_valid_password_imports_backup_and_navigates_to_main() {
    let mut engine = fresh_engine();
    let _ = engine.navigate_to(AppScreen::Onboarding);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore_backup".into(),
    });
    let _ = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: make_backup("correct horse battery staple", "Bob"),
        filename: "bob-backup.txt".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_password".into(),
        value: "correct horse battery staple".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_backup_password".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "my_info");
        }
        other => panic!("expected NavigateTo(my_info), got {other:?}"),
    }
    assert!(
        engine.vauchi().has_identity(),
        "identity should be restored after import"
    );
}

// @scenario: core_file_picker :: wrong password surfaces alert
#[test]
fn submit_wrong_password_returns_alert_and_clears_state() {
    let mut engine = fresh_engine();
    let _ = engine.navigate_to(AppScreen::Onboarding);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore_backup".into(),
    });
    let _ = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: make_backup("correct horse battery staple", "Eve"),
        filename: "eve-backup.txt".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_password".into(),
        value: "wrong".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_backup_password".into(),
    });

    assert!(
        matches!(result, ActionResult::ShowAlert { .. }),
        "wrong password must produce ShowAlert, got {result:?}"
    );
    assert!(
        !engine.vauchi().has_identity(),
        "identity must NOT be created on wrong-password import"
    );
}

// @scenario: core_file_picker :: empty password is rejected before import
#[test]
fn submit_empty_password_surfaces_validation_error_in_component() {
    use vauchi_app::ui::Component;
    let mut engine = fresh_engine();
    let _ = engine.navigate_to(AppScreen::Onboarding);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore_backup".into(),
    });
    let _ = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: make_backup("correct horse battery staple", "Eve"),
        filename: "eve-backup.txt".into(),
    });
    // Submit without typing a password. AppEngine's routing layer
    // re-injects ValidationError into the matching component as
    // `validation_error` and re-emits the screen — this is the
    // pre-existing pattern (`ActionResult::ValidationError` is never
    // returned to the frontend; see `action.rs:111`).
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_backup_password".into(),
    });

    let screen = match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("expected UpdateScreen with validation_error, got {other:?}"),
    };
    let backup_password_component = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::TextInput {
                id,
                validation_error,
                ..
            } if id == "backup_password" => Some(validation_error.clone()),
            _ => None,
        })
        .expect("backup_password TextInput missing from screen");
    let err = backup_password_component.expect("validation_error must be set on empty submit");
    assert!(
        err.to_lowercase().contains("password"),
        "validation_error should mention the password field, got {err:?}"
    );
}

// @scenario: core_file_picker :: back from password entry clears pending bytes
#[test]
fn back_from_password_entry_clears_pending_state() {
    let mut engine = fresh_engine();
    let _ = engine.navigate_to(AppScreen::Onboarding);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore_backup".into(),
    });
    let _ = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: make_backup("correct horse battery staple", "Eve"),
        filename: "eve-backup.txt".into(),
    });

    // Tap "back" — should land on link_choice.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) | ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "link_choice");
        }
        other => panic!("expected NavigateTo(link_choice), got {other:?}"),
    }
}

// Note: the lost-device end-to-end test through AppEngine isn't
// exercised here. AppScreen::DeviceReplacement navigates via
// `DeviceReplacementEngine::new_source()` (Source role / ShowQr step
// — for the OLD device setting up a NEW one). The Target role /
// SelectMode where "lost_device" is handled is reached via a
// different navigation path that's out of scope for Phase 2B's
// scope. The unit-level emit-FilePickFromUser behaviour is covered
// by `device_replacement::tests::select_mode_lost_device_emits_
// file_pick_for_backup` (inline tests).
