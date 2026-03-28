// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the fingerprint verification workflow engine.
//!
//! Verifies that the engine displays both fingerprints and handles
//! the confirm action correctly.

use vauchi_app::ui::{
    ActionResult, FingerprintVerifyEngine, UserAction, VerifyAction, WorkflowEngine,
};

// @scenario: fingerprint.feature - Fingerprint screen shows both fingerprints
#[test]
fn test_fingerprint_screen_shows_both_fingerprints() {
    let engine = FingerprintVerifyEngine::new(
        "contact-123",
        "AB12 CD34 EF56 7890",
        "1234 5678 9ABC DEF0",
        false,
    );

    let screen = engine.current_screen();
    let screen_json = serde_json::to_string(&screen).unwrap();

    assert!(
        screen_json.contains("AB12 CD34 EF56 7890"),
        "Screen must show their fingerprint"
    );
    assert!(
        screen_json.contains("1234 5678 9ABC DEF0"),
        "Screen must show our fingerprint"
    );
}

// @scenario: fingerprint.feature - Confirm match marks contact verified
#[test]
fn test_confirm_match_sets_verified_action() {
    let mut engine = FingerprintVerifyEngine::new(
        "contact-123",
        "AB12 CD34 EF56 7890",
        "1234 5678 9ABC DEF0",
        false,
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_match".into(),
    });

    assert_eq!(result, ActionResult::Complete);
    assert_eq!(
        engine.completion_action(),
        VerifyAction::Verified,
        "Confirm must set Verified — routing calls verify_contact_fingerprint"
    );
}

// @scenario: fingerprint.feature - Already verified shows status
#[test]
fn test_already_verified_shows_status() {
    let engine = FingerprintVerifyEngine::new(
        "contact-123",
        "AB12 CD34 EF56 7890",
        "1234 5678 9ABC DEF0",
        true, // already verified
    );

    let screen = engine.current_screen();
    let screen_json = serde_json::to_string(&screen).unwrap();

    assert!(
        screen_json.contains("erified"),
        "Screen must indicate already verified"
    );
}

// @scenario: fingerprint.feature - Back navigates away without verifying
#[test]
fn test_back_sets_none_action() {
    let mut engine = FingerprintVerifyEngine::new(
        "contact-123",
        "AB12 CD34 EF56 7890",
        "1234 5678 9ABC DEF0",
        false,
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    assert_eq!(result, ActionResult::Complete);
    assert_eq!(
        engine.completion_action(),
        VerifyAction::None,
        "Back must set None — routing does nothing"
    );
}

// @scenario: fingerprint.feature - Unverify removes verified status
#[test]
fn test_unverify_sets_unverified_action() {
    let mut engine = FingerprintVerifyEngine::new(
        "contact-123",
        "AB12 CD34 EF56 7890",
        "1234 5678 9ABC DEF0",
        true, // already verified
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unverify".into(),
    });

    assert_eq!(result, ActionResult::Complete);
    assert_eq!(
        engine.completion_action(),
        VerifyAction::Unverified,
        "Unverify must set Unverified — routing calls unverify_contact_fingerprint"
    );
}

// @scenario: fingerprint.feature - Verified screen shows unverify button
#[test]
fn test_verified_screen_shows_unverify_button() {
    let engine = FingerprintVerifyEngine::new(
        "contact-123",
        "AB12 CD34 EF56 7890",
        "1234 5678 9ABC DEF0",
        true,
    );

    let screen = engine.current_screen();
    let has_unverify = screen.actions.iter().any(|a| a.id == "unverify");
    assert!(has_unverify, "Verified screen must have an unverify button");
}

// @scenario: fingerprint.feature - Confirm ignored when already verified
#[test]
fn test_confirm_ignored_when_already_verified() {
    let mut engine = FingerprintVerifyEngine::new(
        "contact-123",
        "AB12 CD34 EF56 7890",
        "1234 5678 9ABC DEF0",
        true, // already verified
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_match".into(),
    });

    // Should not set Verified (idempotent no-op), just update screen
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "confirm_match when already verified should be a no-op"
    );
}
