// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the ADR-031 file-picker wiring.
//!
//! Phase 2A of `2026-05-03-core-file-picker-command`: when the user
//! triggers `import_contacts` on the More screen, AppEngine emits an
//! `ExchangeCommand::FilePickFromUser`. The frontend opens the native
//! picker, then sends `FilePickedFromUser{bytes, filename}` back. Core
//! routes the bytes by current screen and calls
//! `Vauchi::import_contacts_from_vcf`, returning a toast with the
//! imported / skipped counts.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::{ExchangeCommand, ExchangeHardwareEvent, FilePickPurpose};

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
        ActionResult::ExchangeCommands { commands } => {
            assert_eq!(
                commands.len(),
                1,
                "expected one command, got {:?}",
                commands
            );
            match &commands[0] {
                ExchangeCommand::FilePickFromUser {
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
        other => panic!("expected ExchangeCommands, got {:?}", other),
    }
}

// @scenario: core_file_picker :: FilePickedFromUser on More imports vCard
#[test]
fn file_picked_from_more_imports_vcards() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::More);

    let result = engine.handle_hardware_event(ExchangeHardwareEvent::FilePickedFromUser {
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

    let _ = engine.handle_hardware_event(ExchangeHardwareEvent::FilePickedFromUser {
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
    let result = engine.handle_hardware_event(ExchangeHardwareEvent::FilePickedFromUser {
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

    let result = engine.handle_hardware_event(ExchangeHardwareEvent::FilePickCancelledByUser);
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

    let result = engine.handle_hardware_event(ExchangeHardwareEvent::FilePickedFromUser {
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
    let _ = engine.handle_hardware_event(ExchangeHardwareEvent::FilePickedFromUser {
        bytes: VCF_WITH_UID.to_vec(),
        filename: "contacts.vcf".into(),
    });

    // Import the same vCards again — both should be skipped (UID match).
    let result = engine.handle_hardware_event(ExchangeHardwareEvent::FilePickedFromUser {
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
