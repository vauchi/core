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
//! - API integration via Vauchi

use vauchi_core::network::MockTransport;
use vauchi_core::onboarding::{display_name_suggestions, OnboardingProgress, OnboardingStep};
use vauchi_core::Vauchi;

fn create_test_vauchi() -> Vauchi<MockTransport> {
    Vauchi::in_memory().unwrap()
}

// =============================================================================
// OnboardingStep Tests
// =============================================================================

// @scenario: onboarding:step_order
#[test]
fn test_step_ordering_matches_wizard_flow() {
    let all = OnboardingStep::all();
    assert_eq!(all.len(), 9, "There should be 9 onboarding steps");
    assert_eq!(all[0], OnboardingStep::Welcome);
    assert_eq!(all[1], OnboardingStep::DefaultName);
    assert_eq!(all[2], OnboardingStep::SkipGate);
    assert_eq!(all[3], OnboardingStep::GroupsSetup);
    assert_eq!(all[4], OnboardingStep::ContactInfo);
    assert_eq!(all[5], OnboardingStep::PreviewCard);
    assert_eq!(all[6], OnboardingStep::SecurityExplanation);
    assert_eq!(all[7], OnboardingStep::BackupPrompt);
    assert_eq!(all[8], OnboardingStep::Ready);
}

// @scenario: onboarding:step_index
#[test]
fn test_step_index_is_zero_based() {
    assert_eq!(OnboardingStep::Welcome.index(), 0);
    assert_eq!(OnboardingStep::DefaultName.index(), 1);
    assert_eq!(OnboardingStep::SkipGate.index(), 2);
    assert_eq!(OnboardingStep::GroupsSetup.index(), 3);
    assert_eq!(OnboardingStep::ContactInfo.index(), 4);
    assert_eq!(OnboardingStep::PreviewCard.index(), 5);
    assert_eq!(OnboardingStep::SecurityExplanation.index(), 6);
    assert_eq!(OnboardingStep::BackupPrompt.index(), 7);
    assert_eq!(OnboardingStep::Ready.index(), 8);
}

// @scenario: onboarding:step_navigation
#[test]
fn test_step_next_and_previous() {
    // Welcome has no previous
    assert_eq!(OnboardingStep::Welcome.previous(), None);
    assert_eq!(
        OnboardingStep::Welcome.next(),
        Some(OnboardingStep::DefaultName)
    );

    // Middle step has both
    assert_eq!(
        OnboardingStep::ContactInfo.previous(),
        Some(OnboardingStep::GroupsSetup)
    );
    assert_eq!(
        OnboardingStep::ContactInfo.next(),
        Some(OnboardingStep::PreviewCard)
    );

    // Ready has no next
    assert_eq!(
        OnboardingStep::Ready.previous(),
        Some(OnboardingStep::BackupPrompt)
    );
    assert_eq!(OnboardingStep::Ready.next(), None);
}

// =============================================================================
// OnboardingProgress Tests
// =============================================================================

// @scenario: onboarding:new_progress (#1)
#[test]
fn test_new_progress_starts_at_welcome() {
    let progress = OnboardingProgress::new();

    assert_eq!(
        progress.current_step(),
        OnboardingStep::Welcome,
        "New progress should start at Welcome"
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

    // Advance from Welcome to DefaultName
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::DefaultName);
    assert!(progress.completed_steps.contains(&OnboardingStep::Welcome));

    // Advance through remaining steps
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::SkipGate);
    assert!(progress
        .completed_steps
        .contains(&OnboardingStep::DefaultName));

    let step = progress.advance();
    assert_eq!(step, OnboardingStep::GroupsSetup);

    let step = progress.advance();
    assert_eq!(step, OnboardingStep::ContactInfo);

    let step = progress.advance();
    assert_eq!(step, OnboardingStep::PreviewCard);

    let step = progress.advance();
    assert_eq!(step, OnboardingStep::SecurityExplanation);

    let step = progress.advance();
    assert_eq!(step, OnboardingStep::BackupPrompt);

    let step = progress.advance();
    assert_eq!(step, OnboardingStep::Ready);

    // Verify state before final advance
    assert!(!progress.is_complete());

    // Final advance at Ready marks completion
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::Ready, "Should stay at Ready");
    assert!(
        progress.is_complete(),
        "Should be complete after advancing past Ready"
    );
    assert!(
        progress.completed_at.is_some(),
        "completed_at should be set"
    );

    // Verify all steps are completed
    assert_eq!(progress.completed_steps.len(), 9);
}

// @scenario: onboarding:skip_step (#24)
#[test]
fn test_skip_step_does_not_mark_completed() {
    let mut progress = OnboardingProgress::new();

    // Skip Welcome step
    let step = progress.skip_step();
    assert_eq!(
        step,
        OnboardingStep::DefaultName,
        "Should advance to DefaultName"
    );
    assert!(
        !progress.completed_steps.contains(&OnboardingStep::Welcome),
        "Skipped step should not be in completed_steps"
    );
}

// @scenario: onboarding:skip_backup (#12)
#[test]
fn test_skip_backup_records_flag() {
    let mut progress = OnboardingProgress::new();

    // Advance to BackupPrompt
    progress.advance(); // Welcome -> DefaultName
    progress.advance(); // DefaultName -> SkipGate
    progress.advance(); // SkipGate -> GroupsSetup
    progress.advance(); // GroupsSetup -> ContactInfo
    progress.advance(); // ContactInfo -> PreviewCard
    progress.advance(); // PreviewCard -> SecurityExplanation
    progress.advance(); // SecurityExplanation -> BackupPrompt

    assert_eq!(progress.current_step(), OnboardingStep::BackupPrompt);
    assert!(!progress.skipped_backup);

    // Skip the backup step
    let step = progress.skip_step();
    assert_eq!(step, OnboardingStep::Ready);
    assert!(
        progress.skipped_backup,
        "skipped_backup should be true after skipping BackupPrompt"
    );
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::BackupPrompt),
        "BackupPrompt should not be in completed_steps when skipped"
    );
}

// @scenario: onboarding:completion_percentage (#22)
#[test]
fn test_completion_percentage() {
    let mut progress = OnboardingProgress::new();

    // 0 completed out of 9 total = 0%
    assert_eq!(progress.completion_percentage(), 0);

    // Complete 1 step: 1/9 = 11%
    progress.advance(); // Welcome -> DefaultName, Welcome completed
    assert_eq!(progress.completion_percentage(), 11);

    // Complete all steps
    progress.advance(); // DefaultName -> SkipGate
    progress.advance(); // SkipGate -> GroupsSetup
    progress.advance(); // GroupsSetup -> ContactInfo
    progress.advance(); // ContactInfo -> PreviewCard
    progress.advance(); // PreviewCard -> SecurityExplanation
    progress.advance(); // SecurityExplanation -> BackupPrompt
    progress.advance(); // BackupPrompt -> Ready
    assert_eq!(progress.completion_percentage(), 88); // 8/9

    // Final advance completes Ready
    progress.advance();
    assert_eq!(progress.completion_percentage(), 100); // 9/9
}

// @scenario: onboarding:idempotent_advance_at_final (#26)
#[test]
fn test_idempotent_advance_at_final_step() {
    let mut progress = OnboardingProgress::new();

    // Advance through all steps
    for _ in 0..9 {
        progress.advance();
    }

    assert!(progress.is_complete());
    let completed_at = progress.completed_at;

    // Advance again should be idempotent
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::Ready, "Should stay at Ready");
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

    assert_eq!(progress.current_step(), OnboardingStep::Welcome);
    assert!(progress.completed_steps.is_empty());
    assert!(!progress.skipped_backup);
    assert!(progress.started_at.is_some(), "started_at should be reset");
    assert!(
        progress.completed_at.is_none(),
        "completed_at should be cleared"
    );
    assert!(!progress.is_complete());
}

// @scenario: onboarding:skip_to_finish (#skip_gate)
#[test]
fn test_skip_to_finish_jumps_to_security() {
    let mut progress = OnboardingProgress::new();

    // Advance to SkipGate
    progress.advance(); // Welcome -> DefaultName
    progress.advance(); // DefaultName -> SkipGate

    assert_eq!(progress.current_step(), OnboardingStep::SkipGate);

    // Skip to finish jumps directly to SecurityExplanation
    progress.skip_to_finish();
    assert_eq!(progress.current_step(), OnboardingStep::SecurityExplanation);

    // Intermediate steps (GroupsSetup, ContactInfo, PreviewCard) should NOT be completed
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::GroupsSetup),
        "GroupsSetup should not be completed after skip_to_finish"
    );
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::ContactInfo),
        "ContactInfo should not be completed after skip_to_finish"
    );
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::PreviewCard),
        "PreviewCard should not be completed after skip_to_finish"
    );

    // SkipGate itself should NOT be completed (skip_to_finish doesn't mark it)
    assert!(
        !progress.completed_steps.contains(&OnboardingStep::SkipGate),
        "SkipGate should not be completed after skip_to_finish"
    );

    // Can continue from SecurityExplanation normally
    let step = progress.advance();
    assert_eq!(step, OnboardingStep::BackupPrompt);
}

// @scenario: onboarding:serde_backward_compat
#[test]
fn test_serde_backward_compat_aliases() {
    // JSON with old variant names should deserialize correctly
    let old_json = r#"{"current_step":"CreateIdentity","completed_steps":["AddFields"],"started_at":1000,"completed_at":null,"skipped_backup":false}"#;
    let progress = OnboardingProgress::from_json(old_json).expect("Old JSON should deserialize");
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);
    assert!(progress
        .completed_steps
        .contains(&OnboardingStep::ContactInfo));
}

// @scenario: onboarding:json_serialization_roundtrip (#25)
#[test]
fn test_json_serialization_roundtrip() {
    let mut progress = OnboardingProgress::new();
    progress.advance(); // Welcome -> DefaultName
    progress.advance(); // DefaultName -> SkipGate
    progress.skip_step(); // Skip SkipGate -> GroupsSetup

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
    for _ in 0..9 {
        progress.advance();
    }
    assert!(progress.completed_at.is_some());

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
    progress.advance(); // Welcome -> DefaultName
    progress.advance(); // DefaultName -> SkipGate

    storage.save_onboarding_progress(&progress).unwrap();
    let loaded = storage.load_onboarding_progress().unwrap().unwrap();

    assert_eq!(loaded.current_step(), OnboardingStep::SkipGate);
    assert_eq!(loaded.completed_steps.len(), 2);
    assert!(loaded.completed_steps.contains(&OnboardingStep::Welcome));
    assert!(loaded
        .completed_steps
        .contains(&OnboardingStep::DefaultName));
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
    assert_eq!(progress.current_step(), OnboardingStep::Welcome);

    // Save and reload
    let mut progress = progress;
    progress.advance();
    storage.save_onboarding_progress(&progress).unwrap();

    let loaded = storage.load_or_create_onboarding_progress().unwrap();
    assert_eq!(loaded.current_step(), OnboardingStep::DefaultName);
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

// =============================================================================
// Vauchi API Integration Tests
// =============================================================================

// @scenario: onboarding:api_advance (#26)
#[test]
fn test_api_get_and_advance_onboarding() {
    let vauchi = create_test_vauchi();

    // Get initial progress
    let progress = vauchi.get_onboarding_progress().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::Welcome);

    // Advance
    let progress = vauchi.advance_onboarding().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);

    // Progress persists
    let progress = vauchi.get_onboarding_progress().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);
}

// @scenario: onboarding:api_skip (#24)
#[test]
fn test_api_skip_onboarding_step() {
    let vauchi = create_test_vauchi();

    let progress = vauchi.skip_onboarding_step().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);

    // Skipped step should not be in completed_steps
    assert!(
        !progress.completed_steps.contains(&OnboardingStep::Welcome),
        "Skipped Welcome should not be completed"
    );
}

// @scenario: onboarding:api_reset (#28)
#[test]
fn test_api_reset_onboarding() {
    let vauchi = create_test_vauchi();

    // Advance a few steps
    vauchi.advance_onboarding().unwrap();
    vauchi.advance_onboarding().unwrap();

    // Reset
    vauchi.reset_onboarding().unwrap();

    let progress = vauchi.get_onboarding_progress().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::Welcome);
    assert!(progress.completed_steps.is_empty());
}

// @scenario: onboarding:api_is_complete (#26)
#[test]
fn test_api_is_onboarding_complete() {
    let vauchi = create_test_vauchi();

    assert!(
        !vauchi.is_onboarding_complete().unwrap(),
        "Should not be complete initially"
    );

    // Advance through all steps
    for _ in 0..9 {
        vauchi.advance_onboarding().unwrap();
    }

    assert!(
        vauchi.is_onboarding_complete().unwrap(),
        "Should be complete after advancing through all steps"
    );
}

// @scenario: onboarding:api_completion_percentage (#22)
#[test]
fn test_api_completion_percentage() {
    let vauchi = create_test_vauchi();

    assert_eq!(vauchi.onboarding_completion_percentage().unwrap(), 0);

    vauchi.advance_onboarding().unwrap();
    assert_eq!(vauchi.onboarding_completion_percentage().unwrap(), 11); // 1/9

    // Advance through all
    for _ in 0..8 {
        vauchi.advance_onboarding().unwrap();
    }
    assert_eq!(vauchi.onboarding_completion_percentage().unwrap(), 100);
}

// @scenario: onboarding:api_current_step
#[test]
fn test_api_current_onboarding_step() {
    let vauchi = create_test_vauchi();

    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::Welcome
    );

    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::DefaultName
    );
}

// @scenario: onboarding:api_skip_to_finish (#skip_gate)
#[test]
fn test_api_skip_onboarding_to_finish() {
    let vauchi = create_test_vauchi();

    // Advance to SkipGate
    vauchi.advance_onboarding().unwrap(); // Welcome -> DefaultName
    vauchi.advance_onboarding().unwrap(); // DefaultName -> SkipGate

    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::SkipGate
    );

    // Skip to finish
    let progress = vauchi.skip_onboarding_to_finish().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::SecurityExplanation);

    // Verify it persisted
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::SecurityExplanation
    );
}
