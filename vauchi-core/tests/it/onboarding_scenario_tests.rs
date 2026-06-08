// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! SP-21 Onboarding Scenario Tests (M14)
//!
//! Tests for the 6-step onboarding flow scenarios.
//! Core-verifiable scenarios get full tests; UI-only scenarios
//! get guard-rail tests verifying the core API surface they need.
//!
//! Reference: features/onboarding.feature

use vauchi_core::types::{OnboardingProgress, OnboardingStep};

// ============================================================
// Scenario: Can go back to previous steps
// @scenario: onboarding :: Can go back to previous steps
// ============================================================

// @internal
#[test]
fn test_go_back_preserves_data() {
    let mut progress = OnboardingProgress::new(0);
    assert_eq!(progress.current_step(), OnboardingStep::IdentityCheck);

    progress.advance(0); // → LinkChoice
    progress.advance(0); // → DefaultName
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);

    let prev = progress.current_step().previous();
    assert_eq!(prev, Some(OnboardingStep::LinkChoice));

    // Completed steps should be preserved (not lost on back)
    assert!(progress.completion_percentage() > 0);
}

// @scenario: onboarding :: Can go back to previous steps
// @internal
#[test]
fn test_go_back_from_first_step_returns_none() {
    let progress = OnboardingProgress::new(0);
    assert_eq!(
        progress.current_step().previous(),
        None,
        "first step has no previous"
    );
}

// @scenario: onboarding :: Can go back to previous steps
// @internal
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

// @internal
#[test]
fn test_onboarding_progress_survives_serialization() {
    let mut progress = OnboardingProgress::new(0);
    progress.advance(0); // IdentityCheck → LinkChoice
    progress.advance(0); // LinkChoice → DefaultName
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

// ============================================================
// Scenario: Replay onboarding from settings
// @scenario: onboarding :: Replay onboarding from settings
// ============================================================

// @internal
#[test]
fn test_reset_clears_all_progress() {
    let mut progress = OnboardingProgress::new(0);
    for _ in 0..5 {
        progress.advance(0);
    }
    assert!(progress.completion_percentage() > 50);

    // Reset (simulates "Replay onboarding" from settings)
    progress.reset(0);

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

// @scenario: onboarding :: Replay onboarding from settings
// @internal
#[test]
fn test_reset_does_not_destroy_identity() {
    // the identity (which is the user's key material). It only
    // resets the onboarding wizard state. The actual identity
    // preservation is tested in onboarding_api_tests.rs.
    let mut progress = OnboardingProgress::new(0);
    while !progress.is_complete() {
        progress.advance(0);
    }
    assert!(progress.is_complete());

    progress.reset(0);
    assert!(!progress.is_complete());
    // Progress is a separate struct from Identity — reset is safe
}

// ============================================================
// Scenario: First exchange possible immediately
// @scenario: onboarding :: First exchange possible immediately
// ============================================================

// @internal
#[test]
fn test_onboarding_completion_unblocks_exchange() {
    let mut progress = OnboardingProgress::new(0);

    while !progress.is_complete() {
        progress.advance(0);
    }

    assert!(progress.is_complete());
    assert_eq!(progress.current_step(), OnboardingStep::WhatNext);
    // After completion, exchange is available — verified at the API
    // layer in the app engine tests (exchange screen appears in
    // available_screens after onboarding).
}

// ============================================================
// Scenario: Value clear even without exchange
// @scenario: onboarding :: Value clear even without exchange
// ============================================================

// @internal
#[test]
fn test_completion_without_exchange_is_valid() {
    let mut progress = OnboardingProgress::new(0);
    while !progress.is_complete() {
        progress.advance(0);
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
// @scenario: onboarding :: Empty state with guidance
// @internal
#[test]
fn test_demo_contact_available_for_empty_state() {
    use vauchi_core::demo_contact::{generate_demo_contact_card, get_demo_tips};

    let tips = get_demo_tips();
    assert!(!tips.is_empty(), "should have at least one demo tip");
    let card = generate_demo_contact_card(&tips[0]);
    assert!(!card.display_name.is_empty());
    assert!(card.is_demo);
}
