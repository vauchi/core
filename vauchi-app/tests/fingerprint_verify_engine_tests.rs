// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the fingerprint verification workflow engine.
//!
//! Verifies that the engine displays both fingerprints and handles
//! the confirm action correctly.

use vauchi_app::ui::{ActionResult, FingerprintVerifyEngine, UserAction, WorkflowEngine};

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
fn test_confirm_match_completes_not_cancelled() {
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
    assert!(
        !engine.was_cancelled(),
        "Confirm must NOT set cancelled — routing uses this to call verify_contact_fingerprint"
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
fn test_back_completes_with_cancelled() {
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
    assert!(
        engine.was_cancelled(),
        "Back must set cancelled — routing uses this to skip verify_contact_fingerprint"
    );
}
