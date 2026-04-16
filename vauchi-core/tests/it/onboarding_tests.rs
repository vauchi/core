// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the onboarding state machine (SP-21).
//!
//! Traces to: features/onboarding.feature
//!
//! Covers:
//! - Step progression through all steps (#26)
//! - Skip behavior (#24)
//! - Persistence / save + load roundtrip (#25)
//! - Reset (#28)
//! - Completion percentage (#22)
//! - Idempotent advance at final step (#26)
//! - Display name suggestions (#9)
//! - Backward transitions (#23)
//! - JSON serialization roundtrip

use vauchi_core::onboarding::display_name_suggestions;
use vauchi_core::types::{OnboardingProgress, OnboardingStep};

// =============================================================================
// OnboardingStep Tests
// =============================================================================

// @scenario: onboarding:step_order
#[test]
fn test_step_ordering_matches_wizard_flow() {
    let all = OnboardingStep::all();
    assert_eq!(all.len(), 6, "There should be 6 onboarding steps");
    assert_eq!(all[0], OnboardingStep::IdentityCheck);
    assert_eq!(all[1], OnboardingStep::LinkChoice);
    assert_eq!(all[2], OnboardingStep::DefaultName);
    assert_eq!(all[3], OnboardingStep::GroupsSetup);
    assert_eq!(all[4], OnboardingStep::ContactInfo);
    assert_eq!(all[5], OnboardingStep::WhatNext);
}

// @scenario: onboarding:step_index
#[test]
fn test_step_index_is_zero_based() {
    assert_eq!(OnboardingStep::IdentityCheck.index(), 0);
    assert_eq!(OnboardingStep::LinkChoice.index(), 1);
    assert_eq!(OnboardingStep::DefaultName.index(), 2);
    assert_eq!(OnboardingStep::GroupsSetup.index(), 3);
    assert_eq!(OnboardingStep::ContactInfo.index(), 4);
    assert_eq!(OnboardingStep::WhatNext.index(), 5);
}

// @scenario: onboarding:step_navigation
#[test]
fn test_step_next_and_previous() {
    // IdentityCheck has no previous
    assert_eq!(OnboardingStep::IdentityCheck.previous(), None);
    assert_eq!(
        OnboardingStep::IdentityCheck.next(),
        Some(OnboardingStep::LinkChoice)
    );

    // DefaultName.previous() = LinkChoice
    assert_eq!(
        OnboardingStep::DefaultName.previous(),
        Some(OnboardingStep::LinkChoice)
    );
    assert_eq!(
        OnboardingStep::DefaultName.next(),
        Some(OnboardingStep::GroupsSetup)
    );

    // Middle step has both
    assert_eq!(
        OnboardingStep::ContactInfo.previous(),
        Some(OnboardingStep::GroupsSetup)
    );
    assert_eq!(
        OnboardingStep::ContactInfo.next(),
        Some(OnboardingStep::WhatNext)
    );

    // WhatNext has no next
    assert_eq!(
        OnboardingStep::WhatNext.previous(),
        Some(OnboardingStep::ContactInfo)
    );
    assert_eq!(OnboardingStep::WhatNext.next(), None);
}

// @scenario: onboarding:step_index_consistency
#[test]
fn test_step_index_consistent_with_all() {
    for (expected_idx, step) in OnboardingStep::all().iter().enumerate() {
        assert_eq!(
            step.index(),
            expected_idx,
            "{:?}.index() does not match its position in all()",
            step
        );
    }
}

// =============================================================================
// OnboardingProgress Tests
// =============================================================================

// @scenario: onboarding:new_progress (#1)
#[test]
fn test_new_progress_starts_at_identity_check() {
    let progress = OnboardingProgress::new();

    assert_eq!(
        progress.current_step(),
        OnboardingStep::IdentityCheck,
        "New progress should start at IdentityCheck"
    );
    assert!(
        progress.started_at.is_some(),
        "started_at should be set on creation"
    );
    assert!(
        progress.completed_at.is_none(),
        "completed_at should be None initially"
    );
    assert!(
        progress.completed_steps.is_empty(),
        "No steps should be completed initially"
    );
    assert!(
        !progress.skipped_backup,
        "Backup should not be skipped initially"
    );
    assert!(
        !progress.is_complete(),
        "Onboarding should not be complete initially"
    );
}

// @scenario: onboarding:advance_through_all_steps (#26)
#[test]
fn test_advance_through_all_steps() {
    let mut progress = OnboardingProgress::new();

    // Advance from IdentityCheck to LinkChoice
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::LinkChoice);
    assert!(
        progress
            .completed_steps
            .contains(&OnboardingStep::IdentityCheck)
    );

    // Advance from LinkChoice to DefaultName
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::DefaultName);
    assert!(
        progress
            .completed_steps
            .contains(&OnboardingStep::LinkChoice)
    );

    // Advance from DefaultName to GroupsSetup
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::GroupsSetup);
    assert!(
        progress
            .completed_steps
            .contains(&OnboardingStep::DefaultName)
    );

    // Advance through remaining steps
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::ContactInfo);

    let step = progress.advance();
    assert_eq!(step, OnboardingStep::WhatNext);

    // Verify state before final advance
    assert!(!progress.is_complete());

    // Final advance at WhatNext marks completion
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::WhatNext, "Should stay at WhatNext");
    assert!(
        progress.is_complete(),
        "Should be complete after advancing past WhatNext"
    );
    assert!(
        progress.completed_at.is_some(),
        "completed_at should be set"
    );

    // Verify all steps are completed
    assert_eq!(progress.completed_steps.len(), 6);
}

// @scenario: onboarding:skip_step (#24)
#[test]
fn test_skip_step_does_not_mark_completed() {
    let mut progress = OnboardingProgress::new();

    // Skip IdentityCheck step
    let step = progress.skip_step();
    assert_eq!(
        step,
        OnboardingStep::LinkChoice,
        "Should advance to LinkChoice"
    );
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::IdentityCheck),
        "Skipped step should not be in completed_steps"
    );
}

// @scenario: onboarding:completion_percentage (#22)
#[test]
fn test_completion_percentage() {
    let mut progress = OnboardingProgress::new();

    // 0 completed out of 6 total = 0%
    assert_eq!(progress.completion_percentage(), 0);

    // Complete 1 step: 1/6 = 16%
    progress.advance(); // IdentityCheck -> LinkChoice, IdentityCheck completed
    assert_eq!(progress.completion_percentage(), 16);

    // Complete all steps
    progress.advance(); // LinkChoice -> DefaultName
    progress.advance(); // DefaultName -> GroupsSetup
    progress.advance(); // GroupsSetup -> ContactInfo
    progress.advance(); // ContactInfo -> WhatNext
    assert_eq!(progress.completion_percentage(), 83); // 5/6

    // Final advance completes WhatNext
    progress.advance();
    assert_eq!(progress.completion_percentage(), 100); // 6/6
}

// @scenario: onboarding:idempotent_advance_at_final (#26)
#[test]
fn test_idempotent_advance_at_final_step() {
    let mut progress = OnboardingProgress::new();

    // Advance through all steps
    for _ in 0..6 {
        progress.advance();
    }

    assert!(progress.is_complete());
    let completed_at = progress.completed_at;

    // Advance again should be idempotent
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::WhatNext, "Should stay at WhatNext");
    assert!(progress.is_complete(), "Should still be complete");
    assert_eq!(
        progress.completed_at, completed_at,
        "completed_at should not change on repeated advance"
    );
}

// @scenario: onboarding:reset (#28)
#[test]
fn test_reset_clears_all_progress() {
    let mut progress = OnboardingProgress::new();

    // Advance through several steps
    progress.advance();
    progress.advance();
    progress.advance();

    assert_eq!(progress.completed_steps.len(), 3);

    // Reset
    progress.reset();

    assert_eq!(progress.current_step(), OnboardingStep::IdentityCheck);
    assert!(progress.completed_steps.is_empty());
    assert!(!progress.skipped_backup);
    assert!(progress.started_at.is_some(), "started_at should be reset");
    assert!(
        progress.completed_at.is_none(),
        "completed_at should be cleared"
    );
    assert!(!progress.is_complete());
}

// @scenario: onboarding:serde_backward_compat
#[test]
fn test_serde_backward_compat_aliases() {
    // JSON with old variant names should deserialize correctly
    let old_json = r#"{"current_step":"CreateIdentity","completed_steps":["AddFields"],"started_at":1000,"completed_at":null,"skipped_backup":false}"#;
    let progress = OnboardingProgress::from_json(old_json).expect("Old JSON should deserialize");
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);
    assert!(
        progress
            .completed_steps
            .contains(&OnboardingStep::ContactInfo)
    );

    // Re-serialization must use canonical names, not aliases
    let re_serialized = progress.to_json().expect("Re-serialization should succeed");
    assert!(
        re_serialized.contains("DefaultName"),
        "Canonical name must be serialized"
    );
    assert!(
        re_serialized.contains("ContactInfo"),
        "Canonical name must be serialized"
    );
    assert!(
        !re_serialized.contains("CreateIdentity"),
        "Old alias must not appear in new JSON"
    );
    assert!(
        !re_serialized.contains("AddFields"),
        "Old alias must not appear in new JSON"
    );
}

// @scenario: onboarding:serde_backward_compat_no_identity_check
#[test]
fn test_serde_backward_compat_old_json_without_identity_check() {
    // Users who started onboarding before IdentityCheck was added have
    // persisted JSON with "current_step": "Welcome". This now maps to
    // DefaultName after the 6-step flow change.
    let old_json = r#"{"current_step":"Welcome","completed_steps":[],"started_at":1000,"completed_at":null,"skipped_backup":false}"#;
    let progress = OnboardingProgress::from_json(old_json).expect("Old JSON should deserialize");
    assert_eq!(
        progress.current_step(),
        OnboardingStep::DefaultName,
        "Old JSON starting at Welcome must deserialize to DefaultName"
    );
    assert!(progress.completed_steps.is_empty());
}

// @scenario: onboarding:json_serialization_roundtrip (#25)
#[test]
fn test_json_serialization_roundtrip() {
    let mut progress = OnboardingProgress::new();
    progress.advance(); // IdentityCheck -> LinkChoice
    progress.advance(); // LinkChoice -> DefaultName
    progress.skip_step(); // Skip DefaultName -> GroupsSetup

    let json = progress.to_json().expect("Serialization should succeed");
    let restored = OnboardingProgress::from_json(&json).expect("Deserialization should succeed");

    assert_eq!(restored.current_step(), progress.current_step());
    assert_eq!(restored.completed_steps, progress.completed_steps);
    assert_eq!(restored.started_at, progress.started_at);
    assert_eq!(restored.completed_at, progress.completed_at);
    assert_eq!(restored.skipped_backup, progress.skipped_backup);
}

// @scenario: onboarding:json_roundtrip_with_timestamps
#[test]
fn test_json_roundtrip_preserves_option_timestamps() {
    let mut progress = OnboardingProgress::new();

    // Verify started_at is preserved
    let json = progress.to_json().unwrap();
    let restored = OnboardingProgress::from_json(&json).unwrap();
    assert_eq!(restored.started_at, progress.started_at);

    // Complete and verify completed_at is preserved
    for _ in 0..6 {
        progress.advance();
    }
    progress.completed_at.expect("expected Some");

    let json = progress.to_json().unwrap();
    let restored = OnboardingProgress::from_json(&json).unwrap();
    assert_eq!(
        restored.completed_at, progress.completed_at,
        "completed_at timestamp should survive JSON roundtrip"
    );
}

// @scenario: onboarding:default
#[test]
fn test_default_matches_new() {
    let default_progress = OnboardingProgress::default();
    let new_progress = OnboardingProgress::new();

    assert_eq!(default_progress.current_step(), new_progress.current_step());
    assert_eq!(
        default_progress.completed_steps.len(),
        new_progress.completed_steps.len()
    );
    assert_eq!(default_progress.skipped_backup, new_progress.skipped_backup);
}

// =============================================================================
// Storage Persistence Tests
// =============================================================================

// @scenario: onboarding:storage_roundtrip (#25)
#[test]
fn test_storage_save_load_roundtrip() {
    let storage = vauchi_core::Storage::in_memory(vauchi_core::SymmetricKey::generate()).unwrap();

    let mut progress = OnboardingProgress::new();
    progress.advance(); // IdentityCheck -> LinkChoice
    progress.advance(); // LinkChoice -> DefaultName

    storage.save_onboarding_progress(&progress).unwrap();
    let loaded = storage.load_onboarding_progress().unwrap().unwrap();

    assert_eq!(loaded.current_step(), OnboardingStep::DefaultName);
    assert_eq!(loaded.completed_steps.len(), 2);
    assert!(
        loaded
            .completed_steps
            .contains(&OnboardingStep::IdentityCheck)
    );
    assert!(loaded.completed_steps.contains(&OnboardingStep::LinkChoice));
}

// @scenario: onboarding:storage_load_empty
#[test]
fn test_storage_load_returns_none_when_empty() {
    let storage = vauchi_core::Storage::in_memory(vauchi_core::SymmetricKey::generate()).unwrap();

    let loaded = storage.load_onboarding_progress().unwrap();
    assert!(loaded.is_none(), "Should return None when no state exists");
}

// @scenario: onboarding:storage_load_or_create
#[test]
fn test_storage_load_or_create() {
    let storage = vauchi_core::Storage::in_memory(vauchi_core::SymmetricKey::generate()).unwrap();

    // First call creates new
    let progress = storage.load_or_create_onboarding_progress().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::IdentityCheck);

    // Save and reload
    let mut progress = progress;
    progress.advance();
    storage.save_onboarding_progress(&progress).unwrap();

    let loaded = storage.load_or_create_onboarding_progress().unwrap();
    assert_eq!(loaded.current_step(), OnboardingStep::LinkChoice);
}

// @scenario: onboarding:storage_overwrite
#[test]
fn test_storage_overwrite_replaces_previous() {
    let storage = vauchi_core::Storage::in_memory(vauchi_core::SymmetricKey::generate()).unwrap();

    // Save initial state
    let mut progress = OnboardingProgress::new();
    progress.advance();
    storage.save_onboarding_progress(&progress).unwrap();

    // Save updated state
    progress.advance();
    progress.advance();
    storage.save_onboarding_progress(&progress).unwrap();

    let loaded = storage.load_onboarding_progress().unwrap().unwrap();
    assert_eq!(
        loaded.current_step(),
        OnboardingStep::GroupsSetup,
        "Should have the latest saved state"
    );
}

// =============================================================================
// Display Name Suggestions Tests
// =============================================================================

// @scenario: onboarding:display_name_full_name (#9)
#[test]
fn test_display_name_suggestions_full_name() {
    let suggestions = display_name_suggestions("Alexandra Johnson");

    assert!(
        suggestions.contains(&"Alexandra".to_string()),
        "Should contain first name"
    );
    assert!(
        suggestions.contains(&"Alex".to_string()),
        "Should contain shortened first name"
    );
    assert!(
        suggestions.contains(&"A. Johnson".to_string()),
        "Should contain initial + last name"
    );
}

// @scenario: onboarding:display_name_single (#9)
#[test]
fn test_display_name_suggestions_single_name() {
    // "Alice" has 5 chars, so a shortened version "Alic" is also generated
    let suggestions = display_name_suggestions("Alice");

    assert_eq!(
        suggestions.len(),
        2,
        "5-char single name should produce 2 suggestions"
    );
    assert_eq!(suggestions[0], "Alice");
    assert_eq!(suggestions[1], "Alic");

    // Short name (< 5 chars) produces just 1 suggestion
    let suggestions = display_name_suggestions("Bob");
    assert_eq!(
        suggestions.len(),
        1,
        "Short single name should produce 1 suggestion"
    );
    assert_eq!(suggestions[0], "Bob");
}

// @scenario: onboarding:display_name_short
#[test]
fn test_display_name_suggestions_short_name() {
    let suggestions = display_name_suggestions("Jo Smith");

    assert!(
        suggestions.contains(&"Jo".to_string()),
        "Should contain first name"
    );
    // "Jo" is only 2 chars, no shortened version
    assert!(
        suggestions.contains(&"J. Smith".to_string()),
        "Should contain initial + last name"
    );
    assert_eq!(
        suggestions.len(),
        2,
        "Short first name should not get shortened version"
    );
}

// @scenario: onboarding:display_name_empty
#[test]
fn test_display_name_suggestions_empty() {
    let suggestions = display_name_suggestions("");
    assert!(
        suggestions.is_empty(),
        "Empty input should return no suggestions"
    );

    let suggestions = display_name_suggestions("   ");
    assert!(
        suggestions.is_empty(),
        "Whitespace-only input should return no suggestions"
    );
}

// @scenario: onboarding:display_name_unicode
#[test]
fn test_display_name_suggestions_unicode() {
    let suggestions = display_name_suggestions("Müller Schmidt");

    assert!(
        suggestions.contains(&"Müller".to_string()),
        "Should contain unicode first name"
    );
    assert!(
        suggestions.contains(&"Müll".to_string()),
        "Should contain shortened unicode name (4 chars)"
    );
    assert!(
        suggestions.contains(&"M. Schmidt".to_string()),
        "Should contain initial + last name"
    );
}
