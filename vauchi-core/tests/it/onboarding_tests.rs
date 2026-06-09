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

use rstest::rstest;
use vauchi_core::onboarding::display_name_suggestions;
use vauchi_core::types::{OnboardingProgress, OnboardingStep};

// =============================================================================
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

    assert_eq!(
        OnboardingStep::ContactInfo.previous(),
        Some(OnboardingStep::GroupsSetup)
    );
    assert_eq!(
        OnboardingStep::ContactInfo.next(),
        Some(OnboardingStep::WhatNext)
    );

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
// =============================================================================

// @scenario: onboarding:new_progress (#1)
#[test]
fn test_new_progress_starts_at_identity_check() {
    let progress = OnboardingProgress::new(0);

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
    let mut progress = OnboardingProgress::new(0);

    // Advance from IdentityCheck to LinkChoice
    let step = progress.advance(0);
    assert_eq!(step, OnboardingStep::LinkChoice);
    assert!(
        progress
            .completed_steps
            .contains(&OnboardingStep::IdentityCheck)
    );

    // Advance from LinkChoice to DefaultName
    let step = progress.advance(0);
    assert_eq!(step, OnboardingStep::DefaultName);
    assert!(
        progress
            .completed_steps
            .contains(&OnboardingStep::LinkChoice)
    );

    // Advance from DefaultName to GroupsSetup
    let step = progress.advance(0);
    assert_eq!(step, OnboardingStep::GroupsSetup);
    assert!(
        progress
            .completed_steps
            .contains(&OnboardingStep::DefaultName)
    );

    let step = progress.advance(0);
    assert_eq!(step, OnboardingStep::ContactInfo);

    let step = progress.advance(0);
    assert_eq!(step, OnboardingStep::WhatNext);

    assert!(!progress.is_complete());

    // Final advance at WhatNext marks completion
    let step = progress.advance(0);
    assert_eq!(step, OnboardingStep::WhatNext, "Should stay at WhatNext");
    assert!(
        progress.is_complete(),
        "Should be complete after advancing past WhatNext"
    );
    assert!(
        progress.completed_at.is_some(),
        "completed_at should be set"
    );

    assert_eq!(progress.completed_steps.len(), 6);
}

// @scenario: onboarding:skip_step (#24)
#[test]
fn test_skip_step_does_not_mark_completed() {
    let mut progress = OnboardingProgress::new(0);

    let step = progress.skip_step(0);
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
    let mut progress = OnboardingProgress::new(0);

    // 0 completed out of 6 total = 0%
    assert_eq!(progress.completion_percentage(), 0);

    // Complete 1 step: 1/6 = 16%
    progress.advance(0); // IdentityCheck -> LinkChoice, IdentityCheck completed
    assert_eq!(progress.completion_percentage(), 16);

    progress.advance(0); // LinkChoice -> DefaultName
    progress.advance(0); // DefaultName -> GroupsSetup
    progress.advance(0); // GroupsSetup -> ContactInfo
    progress.advance(0); // ContactInfo -> WhatNext
    assert_eq!(progress.completion_percentage(), 83); // 5/6

    progress.advance(0);
    assert_eq!(progress.completion_percentage(), 100); // 6/6
}

// @scenario: onboarding:idempotent_advance_at_final (#26)
#[test]
fn test_idempotent_advance_at_final_step() {
    let mut progress = OnboardingProgress::new(0);

    for _ in 0..6 {
        progress.advance(0);
    }

    assert!(progress.is_complete());
    let completed_at = progress.completed_at;

    // Advance again should be idempotent
    let step = progress.advance(0);
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
    let mut progress = OnboardingProgress::new(0);

    progress.advance(0);
    progress.advance(0);
    progress.advance(0);

    assert_eq!(progress.completed_steps.len(), 3);

    progress.reset(0);

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
    let mut progress = OnboardingProgress::new(0);
    progress.advance(0); // IdentityCheck -> LinkChoice
    progress.advance(0); // LinkChoice -> DefaultName
    progress.skip_step(0); // Skip DefaultName -> GroupsSetup

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
    let mut progress = OnboardingProgress::new(0);

    let json = progress.to_json().unwrap();
    let restored = OnboardingProgress::from_json(&json).unwrap();
    assert_eq!(restored.started_at, progress.started_at);

    // Complete and verify completed_at is preserved
    for _ in 0..6 {
        progress.advance(0);
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
    let new_progress = OnboardingProgress::new(0);

    assert_eq!(default_progress.current_step(), new_progress.current_step());
    assert_eq!(
        default_progress.completed_steps.len(),
        new_progress.completed_steps.len()
    );
    assert_eq!(default_progress.skipped_backup, new_progress.skipped_backup);
}

// =============================================================================
// =============================================================================

// @scenario: onboarding:storage_roundtrip (#25)
#[test]
fn test_storage_save_load_roundtrip() {
    let storage = vauchi_core::Storage::in_memory(vauchi_core::SymmetricKey::generate()).unwrap();

    let mut progress = OnboardingProgress::new(0);
    progress.advance(0); // IdentityCheck -> LinkChoice
    progress.advance(0); // LinkChoice -> DefaultName

    storage.ux().save_onboarding_progress(&progress).unwrap();
    let loaded = storage.ux().load_onboarding_progress().unwrap().unwrap();

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

    let loaded = storage.ux().load_onboarding_progress().unwrap();
    assert!(loaded.is_none(), "Should return None when no state exists");
}

// @scenario: onboarding:storage_load_or_create
#[test]
fn test_storage_load_or_create() {
    let storage = vauchi_core::Storage::in_memory(vauchi_core::SymmetricKey::generate()).unwrap();

    let progress = storage.ux().load_or_create_onboarding_progress().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::IdentityCheck);

    let mut progress = progress;
    progress.advance(0);
    storage.ux().save_onboarding_progress(&progress).unwrap();

    let loaded = storage.ux().load_or_create_onboarding_progress().unwrap();
    assert_eq!(loaded.current_step(), OnboardingStep::LinkChoice);
}

// @scenario: onboarding:storage_overwrite
#[test]
fn test_storage_overwrite_replaces_previous() {
    let storage = vauchi_core::Storage::in_memory(vauchi_core::SymmetricKey::generate()).unwrap();

    let mut progress = OnboardingProgress::new(0);
    progress.advance(0);
    storage.ux().save_onboarding_progress(&progress).unwrap();

    progress.advance(0);
    progress.advance(0);
    storage.ux().save_onboarding_progress(&progress).unwrap();

    let loaded = storage.ux().load_onboarding_progress().unwrap().unwrap();
    assert_eq!(
        loaded.current_step(),
        OnboardingStep::GroupsSetup,
        "Should have the latest saved state"
    );
}

// =============================================================================
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
// =============================================================================
// Pin specific values so arithmetic / comparison mutations cannot survive.

// @scenario: onboarding:completion_percentage_table
#[rstest]
#[case(0, 0)]
#[case(1, 16)]
#[case(2, 33)]
#[case(3, 50)]
#[case(4, 66)]
#[case(5, 83)]
#[case(6, 100)]
fn test_completion_percentage_exact_per_completed_step(
    #[case] completed: usize,
    #[case] expected_pct: u8,
) {
    // Pinning the percentage at every completed-count value eliminates
    // surviving arithmetic mutations on `(completed * 100) / total`:
    // - `* with +`: 1+100=101/6=16 (matches at 1 only) → fails at 5/6
    // - `* with /`: 1/100/6=0 → fails at any non-zero
    // - `/ with %`: 100%6=4 → fails at 1
    // - `/ with *`: 100*6=600 → clamps to 100 at any non-zero → fails
    // - `100 with 0/1`: 0/total or 1/total → fails everywhere
    let mut progress = OnboardingProgress::new(0);
    for _ in 0..completed {
        progress.advance(0);
    }
    assert_eq!(progress.completion_percentage(), expected_pct);
}

// @scenario: onboarding:total_steps_constant
#[test]
fn test_total_steps_is_six() {
    // Pin OnboardingStep::total() so mutations to `with 0`/`with 1` and
    // `Vec::leak(Vec::new())` for `all()` are caught.
    assert_eq!(OnboardingStep::total(), 6);
    assert_eq!(OnboardingStep::all().len(), 6);
}

// @scenario: onboarding:step_index_per_variant
#[rstest]
#[case(OnboardingStep::IdentityCheck, 0)]
#[case(OnboardingStep::LinkChoice, 1)]
#[case(OnboardingStep::DefaultName, 2)]
#[case(OnboardingStep::GroupsSetup, 3)]
#[case(OnboardingStep::ContactInfo, 4)]
#[case(OnboardingStep::WhatNext, 5)]
fn test_step_index_per_variant(#[case] step: OnboardingStep, #[case] expected_idx: usize) {
    assert_eq!(step.index(), expected_idx);
}

// @scenario: onboarding:step_next_per_variant
#[rstest]
#[case(OnboardingStep::IdentityCheck, Some(OnboardingStep::LinkChoice))]
#[case(OnboardingStep::LinkChoice, Some(OnboardingStep::DefaultName))]
#[case(OnboardingStep::DefaultName, Some(OnboardingStep::GroupsSetup))]
#[case(OnboardingStep::GroupsSetup, Some(OnboardingStep::ContactInfo))]
#[case(OnboardingStep::ContactInfo, Some(OnboardingStep::WhatNext))]
#[case(OnboardingStep::WhatNext, None)]
fn test_step_next_per_variant(
    #[case] step: OnboardingStep,
    #[case] expected: Option<OnboardingStep>,
) {
    // Catches `+ with -` (would underflow on IdentityCheck → panic, caught
    // there too) and `+ with *` (idx*1 = idx → returns same step, caught
    // for every non-final variant).
    assert_eq!(step.next(), expected);
}

// @scenario: onboarding:step_previous_per_variant
#[rstest]
#[case(OnboardingStep::IdentityCheck, None)]
#[case(OnboardingStep::LinkChoice, Some(OnboardingStep::IdentityCheck))]
#[case(OnboardingStep::DefaultName, Some(OnboardingStep::LinkChoice))]
#[case(OnboardingStep::GroupsSetup, Some(OnboardingStep::DefaultName))]
#[case(OnboardingStep::ContactInfo, Some(OnboardingStep::GroupsSetup))]
#[case(OnboardingStep::WhatNext, Some(OnboardingStep::ContactInfo))]
fn test_step_previous_per_variant(
    #[case] step: OnboardingStep,
    #[case] expected: Option<OnboardingStep>,
) {
    // Catches `== with !=` on the `idx == 0` guard, and `- with +/-/...`
    // on the `idx - 1` index expression.
    assert_eq!(step.previous(), expected);
}

// @scenario: onboarding:advance_completed_at_only_set_at_final
#[test]
fn test_advance_does_not_set_completed_at_before_final_step() {
    // Catches `OnboardingProgress::advance -> with Default::default()` for
    // the intermediate steps: a no-op body would leave completed_at unset
    // — but it would also fail to advance current_step, already covered.
    // This adds the converse pin: completed_at must STAY None until the
    // final advance.
    let mut progress = OnboardingProgress::new(0);
    for _ in 0..5 {
        progress.advance(0);
    }
    assert_eq!(progress.current_step(), OnboardingStep::WhatNext);
    assert!(progress.completed_at.is_none());
    assert!(!progress.is_complete());
    progress.advance(0);
    assert!(progress.completed_at.is_some());
    assert!(progress.is_complete());
}

// @scenario: onboarding:skip_step_completed_at_only_set_at_final
#[test]
fn test_skip_does_not_mark_completed_steps() {
    // skip_step must NOT add the current step to completed_steps. This
    // catches a mutation that swaps skip_step's body for advance's.
    let mut progress = OnboardingProgress::new(0);
    progress.skip_step(0);
    progress.skip_step(0);
    assert_eq!(progress.current_step(), OnboardingStep::DefaultName);
    assert!(progress.completed_steps.is_empty());
}

// @scenario: onboarding:display_name_first_letter_present
#[rstest]
#[case("Alexandra Johnson", "A. Johnson")]
#[case("Bob Marley", "B. Marley")]
#[case("Müller Schmidt", "M. Schmidt")]
fn test_display_name_initial_last(#[case] name: &str, #[case] expected: &str) {
    let suggestions = display_name_suggestions(name);
    assert!(
        suggestions.contains(&expected.to_string()),
        "expected {} in {:?}",
        expected,
        suggestions
    );
}

// @scenario: onboarding:display_name_short_threshold
#[rstest]
#[case("Bob", 1)] // 3 chars: just first
#[case("Anne", 1)] // 4 chars: just first (boundary, < 5)
#[case("Alice", 2)] // 5 chars: first + Alic
#[case("Albert", 2)] // 6 chars: first + Albe
fn test_display_name_single_word_count(#[case] name: &str, #[case] expected: usize) {
    // Pins the `>= 5` threshold for the shortened-name branch. Mutations
    // changing it to `< 5` or `> 5` produce different counts.
    let suggestions = display_name_suggestions(name);
    assert_eq!(suggestions.len(), expected, "for {}", name);
}
