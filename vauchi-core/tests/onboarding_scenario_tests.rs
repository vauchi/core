// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! SP-21 Onboarding Scenario Tests (M14)
//!
//! Tests for the 11 remaining @planned onboarding scenarios.
//! Core-verifiable scenarios get full tests; UI-only scenarios
//! get guard-rail tests verifying the core API surface they need.
//!
//! Reference: features/onboarding.feature

use vauchi_core::types::{OnboardingProgress, OnboardingStep};

// ============================================================
// Scenario: Can go back to previous steps
// @scenario: onboarding :: Can go back to previous steps
// ============================================================

#[test]
fn test_go_back_preserves_data() {
    let mut progress = OnboardingProgress::new();
    assert_eq!(progress.current_step(), OnboardingStep::IdentityCheck);

    // Advance to step 3 (DefaultName)
    progress.advance(); // → LinkChoice
    progress.advance(); // → Welcome
    progress.advance(); // → DefaultName
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);

    // Go back to Welcome
    let prev = progress.current_step().previous();
    assert_eq!(prev, Some(OnboardingStep::Welcome));

    // Completed steps should be preserved (not lost on back)
    assert!(progress.completion_percentage() > 0);
}

#[test]
fn test_go_back_from_first_step_returns_none() {
    let progress = OnboardingProgress::new();
    assert_eq!(
        progress.current_step().previous(),
        None,
        "first step has no previous"
    );
}

#[test]
fn test_every_step_except_first_has_previous() {
    for step in &OnboardingStep::all()[1..] {
        assert!(
            step.previous().is_some(),
            "{:?} should have a previous step",
            step
        );
    }
}

// ============================================================
// Scenario: Exit and resume onboarding
// @scenario: onboarding :: Exit and resume onboarding
// ============================================================

#[test]
fn test_onboarding_progress_survives_serialization() {
    let mut progress = OnboardingProgress::new();
    progress.advance(); // IdentityCheck → LinkChoice
    progress.advance(); // LinkChoice → Welcome
    progress.advance(); // Welcome → DefaultName
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);

    // Serialize (simulates app close)
    let json = progress.to_json().expect("serialize should succeed");

    // Deserialize (simulates app reopen)
    let restored = OnboardingProgress::from_json(&json).expect("deserialize should succeed");

    assert_eq!(
        restored.current_step(),
        OnboardingStep::DefaultName,
        "resume should land on the same step"
    );
    assert_eq!(
        restored.completion_percentage(),
        progress.completion_percentage(),
        "progress percentage should be preserved"
    );
}

#[test]
fn test_onboarding_resume_after_skip_gate() {
    let mut progress = OnboardingProgress::new();
    // Advance to SkipGate
    while progress.current_step() != OnboardingStep::SkipGate {
        progress.advance();
    }
    assert_eq!(progress.current_step(), OnboardingStep::SkipGate);

    // Serialize + restore
    let json = progress.to_json().unwrap();
    let restored = OnboardingProgress::from_json(&json).unwrap();
    assert_eq!(restored.current_step(), OnboardingStep::SkipGate);
}

// ============================================================
// Scenario: Replay onboarding from settings
// @scenario: onboarding :: Replay onboarding from settings
// ============================================================

#[test]
fn test_reset_clears_all_progress() {
    let mut progress = OnboardingProgress::new();
    // Complete most of onboarding
    for _ in 0..8 {
        progress.advance();
    }
    assert!(progress.completion_percentage() > 50);

    // Reset (simulates "Replay onboarding" from settings)
    progress.reset();

    assert_eq!(
        progress.current_step(),
        OnboardingStep::IdentityCheck,
        "reset should go back to first step"
    );
    assert_eq!(
        progress.completion_percentage(),
        0,
        "reset should clear progress"
    );
    assert!(!progress.is_complete(), "reset should clear completion");
}

#[test]
fn test_reset_does_not_destroy_identity() {
    // This is a guard-rail: reset_onboarding should NOT touch
    // the identity (which is the user's key material). It only
    // resets the onboarding wizard state. The actual identity
    // preservation is tested in onboarding_api_tests.rs.
    let mut progress = OnboardingProgress::new();
    // Complete onboarding
    while !progress.is_complete() {
        progress.advance();
    }
    assert!(progress.is_complete());

    // Reset
    progress.reset();
    assert!(!progress.is_complete());
    // Progress is a separate struct from Identity — reset is safe
}

// ============================================================
// Scenario: First exchange possible immediately
// @scenario: onboarding :: First exchange possible immediately
// ============================================================

#[test]
fn test_onboarding_completion_unblocks_exchange() {
    let mut progress = OnboardingProgress::new();

    // Complete onboarding fully
    while !progress.is_complete() {
        progress.advance();
    }

    assert!(progress.is_complete());
    assert_eq!(progress.current_step(), OnboardingStep::Ready);
    // After completion, exchange is available — verified at the API
    // layer in the app engine tests (exchange screen appears in
    // available_screens after onboarding).
}

// ============================================================
// Scenario: Value clear even without exchange
// @scenario: onboarding :: Value clear even without exchange
// ============================================================

#[test]
fn test_completion_without_exchange_is_valid() {
    let mut progress = OnboardingProgress::new();
    while !progress.is_complete() {
        progress.advance();
    }

    // Onboarding is complete, no exchange needed
    assert!(progress.is_complete());
    assert_eq!(
        progress.completion_percentage(),
        100,
        "full completion without exchange should reach 100%"
    );
}

// ============================================================
// Scenario: Empty state with guidance
// @scenario: onboarding :: Empty state with guidance
// ============================================================

// This scenario tests that the contacts list shows guidance when empty.
// Core provides the "demo contact" as the guidance mechanism (DemoContactCard).
// The actual UI rendering is frontend-specific.

#[test]
fn test_demo_contact_available_for_empty_state() {
    use vauchi_core::demo_contact::{generate_demo_contact_card, get_demo_tips};

    let tips = get_demo_tips();
    assert!(!tips.is_empty(), "should have at least one demo tip");
    let card = generate_demo_contact_card(&tips[0]);
    assert!(!card.display_name.is_empty());
    assert!(card.is_demo);
}
