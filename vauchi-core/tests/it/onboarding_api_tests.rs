// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi API integration and end-to-end onboarding flow tests (SP-21).
//!
//! Traces to: features/onboarding.feature
//!
//! Covers:
//! - API integration via Vauchi (advance, skip, reset, completion)
//! - End-to-end onboarding flows (with and without skip)
//! - Identity check and link choice engine tests

use vauchi_core::Vauchi;
use vauchi_core::types::OnboardingStep;

fn create_test_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
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
    assert_eq!(progress.current_step(), OnboardingStep::IdentityCheck);

    // Advance
    let progress = vauchi.advance_onboarding().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::LinkChoice);

    // Progress persists
    let progress = vauchi.get_onboarding_progress().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::LinkChoice);
}

// @scenario: onboarding:api_skip (#24)
#[test]
fn test_api_skip_onboarding_step() {
    let vauchi = create_test_vauchi();

    let progress = vauchi.skip_onboarding_step().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::LinkChoice);

    // Skipped step should not be in completed_steps
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::IdentityCheck),
        "Skipped IdentityCheck should not be completed"
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
    assert_eq!(progress.current_step(), OnboardingStep::IdentityCheck);
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
    for _ in 0..11 {
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
    assert_eq!(vauchi.onboarding_completion_percentage().unwrap(), 9); // 1/11

    // Advance through all
    for _ in 0..10 {
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
        OnboardingStep::IdentityCheck
    );

    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::LinkChoice
    );
}

// @scenario: onboarding:create_suggested_groups
#[test]
fn test_create_suggested_groups() {
    let vauchi = create_test_vauchi();

    let created = vauchi
        .create_suggested_groups(&["Family", "Friends"])
        .unwrap();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0].name(), "Family");
    assert_eq!(created[1].name(), "Friends");
}

// @scenario: onboarding:create_suggested_groups_skips_duplicates
#[test]
fn test_create_suggested_groups_skips_duplicates() {
    let vauchi = create_test_vauchi();

    // Create Family first
    vauchi.create_suggested_groups(&["Family"]).unwrap();

    // Create again with overlap — should skip Family, create Friends
    let created = vauchi
        .create_suggested_groups(&["Family", "Friends"])
        .unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name(), "Friends");
}

// @scenario: onboarding:api_skip_to_finish (#skip_gate)
#[test]
fn test_api_skip_onboarding_to_finish() {
    let vauchi = create_test_vauchi();

    // Advance to SkipGate
    vauchi.advance_onboarding().unwrap(); // IdentityCheck -> LinkChoice
    vauchi.advance_onboarding().unwrap(); // LinkChoice -> Welcome
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

// =============================================================================
// End-to-End Onboarding Flow Tests
// =============================================================================

// @scenario: onboarding:e2e_full_flow_with_skip
#[test]
fn test_full_onboarding_flow_with_skip() {
    let mut vauchi = create_test_vauchi();

    // Step 1: IdentityCheck
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::IdentityCheck
    );
    vauchi.advance_onboarding().unwrap();

    // Step 2: LinkChoice
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::LinkChoice
    );
    vauchi.advance_onboarding().unwrap();

    // Step 3: Welcome
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::Welcome
    );
    vauchi.advance_onboarding().unwrap();

    // Step 4: DefaultName — create identity then advance
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::DefaultName
    );
    vauchi.create_identity("Alice").unwrap();
    assert!(
        vauchi.has_identity(),
        "Identity should exist after create_identity"
    );
    assert_eq!(
        vauchi.public_id().unwrap().len(),
        64,
        "Public ID should be a 64-char hex string"
    );
    vauchi.advance_onboarding().unwrap();

    // Step 5: SkipGate — skip to finish
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::SkipGate
    );
    vauchi.skip_onboarding_to_finish().unwrap();

    // Should jump to SecurityExplanation, skipping GroupsSetup/ContactInfo/PreviewCard
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::SecurityExplanation
    );

    // Verify skipped steps are not marked completed
    let progress = vauchi.get_onboarding_progress().unwrap();
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::GroupsSetup),
        "GroupsSetup should not be completed after skip"
    );
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::ContactInfo),
        "ContactInfo should not be completed after skip"
    );
    assert!(
        !progress
            .completed_steps
            .contains(&OnboardingStep::PreviewCard),
        "PreviewCard should not be completed after skip"
    );

    // Can continue normally from SecurityExplanation
    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::BackupPrompt
    );

    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::Ready
    );

    // Final advance completes onboarding
    vauchi.advance_onboarding().unwrap();
    assert!(
        vauchi.is_onboarding_complete().unwrap(),
        "Onboarding should be complete after final advance"
    );
}

// @scenario: onboarding:skip_step_at_ready_completes
#[test]
fn test_skip_step_at_ready_marks_complete() {
    let mut vauchi = create_test_vauchi();

    // Advance to DefaultName (IdentityCheck -> LinkChoice -> Welcome -> DefaultName)
    vauchi.advance_onboarding().unwrap();
    vauchi.advance_onboarding().unwrap();
    vauchi.advance_onboarding().unwrap();
    vauchi.create_identity("Alice").unwrap();

    // Advance through all steps to Ready
    for _ in 0..7 {
        vauchi.advance_onboarding().unwrap();
    }
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::Ready
    );
    assert!(
        !vauchi.is_onboarding_complete().unwrap(),
        "Should not be complete before skip_step at Ready"
    );

    // skip_step at Ready should complete onboarding (else branch in skip_step)
    vauchi.skip_onboarding_step().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::Ready,
        "Should stay at Ready"
    );
    assert!(
        vauchi.is_onboarding_complete().unwrap(),
        "skip_step at Ready should mark onboarding complete"
    );
}

// @scenario: onboarding:skip_to_finish_wrong_step_is_noop
#[test]
fn test_skip_to_finish_from_wrong_step_is_noop() {
    let vauchi = create_test_vauchi();

    // At IdentityCheck — skip_to_finish should be a no-op
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::IdentityCheck
    );
    vauchi.skip_onboarding_to_finish().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::IdentityCheck,
        "skip_to_finish from IdentityCheck should be a no-op"
    );
}

// =============================================================================
// Identity Check Tests
// =============================================================================

// @scenario: onboarding:identity_check_create_new
#[test]
fn test_identity_check_create_new_goes_to_welcome() {
    use vauchi_app::ui::{ActionResult, OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "identity_check");
    assert!(
        screen.progress.is_none(),
        "IdentityCheck should have no progress indicator"
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "welcome");
            let progress = screen
                .progress
                .as_ref()
                .expect("Welcome should have progress");
            assert_eq!(progress.current_step, 1);
            assert_eq!(progress.total_steps, 9);
        }
        other => panic!("Expected NavigateTo, got {other:?}"),
    }
}

// @scenario: onboarding:identity_check_have_identity
#[test]
fn test_identity_check_have_identity_goes_to_link_choice() {
    use vauchi_app::ui::{ActionResult, OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "link_choice");
            assert!(
                screen.progress.is_none(),
                "LinkChoice should have no progress indicator"
            );
        }
        other => panic!("Expected NavigateTo, got {other:?}"),
    }
}

// @scenario: onboarding:link_choice_link_device
#[test]
fn test_link_choice_link_device_returns_start_device_link() {
    use vauchi_app::ui::{ActionResult, OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
    // Navigate to LinkChoice
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "link_device".into(),
    });
    assert!(
        matches!(result, ActionResult::StartDeviceLink),
        "Expected StartDeviceLink, got {result:?}"
    );
}

// @scenario: onboarding:link_choice_restore_backup
#[test]
fn test_link_choice_restore_backup_returns_start_backup_import() {
    use vauchi_app::ui::{ActionResult, OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
    // Navigate to LinkChoice
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore_backup".into(),
    });
    assert!(
        matches!(result, ActionResult::StartBackupImport),
        "Expected StartBackupImport, got {result:?}"
    );
}

// @scenario: onboarding:link_choice_back
#[test]
fn test_link_choice_back_returns_to_identity_check() {
    use vauchi_app::ui::{ActionResult, OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
    // Navigate to LinkChoice
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "identity_check");
        }
        other => panic!("Expected NavigateTo, got {other:?}"),
    }
}

// @scenario: onboarding:welcome_no_restore_backup
#[test]
fn test_welcome_screen_has_no_restore_backup_action() {
    use vauchi_app::ui::{OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
    // Navigate to Welcome via create_new
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "welcome");
    assert_eq!(
        screen.actions.len(),
        1,
        "Welcome should have exactly 1 action"
    );
    assert_eq!(screen.actions[0].id, "get_started");
}

// @scenario: onboarding:identity_check_unknown_action
#[test]
fn test_identity_check_unknown_action_returns_update_screen() {
    use vauchi_app::ui::{ActionResult, OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "Expected UpdateScreen for unknown action, got {result:?}"
    );
}

// @scenario: onboarding:link_choice_unknown_action
#[test]
fn test_link_choice_unknown_action_returns_update_screen() {
    use vauchi_app::ui::{ActionResult, OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
    // Navigate to LinkChoice
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "Expected UpdateScreen for unknown action, got {result:?}"
    );
}

// @scenario: onboarding:e2e_full_flow_without_skip
#[test]
fn test_full_onboarding_flow_without_skip() {
    let mut vauchi = create_test_vauchi();

    // IdentityCheck → LinkChoice
    vauchi.advance_onboarding().unwrap();

    // LinkChoice → Welcome
    vauchi.advance_onboarding().unwrap();

    // Welcome → DefaultName
    vauchi.advance_onboarding().unwrap();

    // DefaultName → SkipGate (create identity first)
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::DefaultName
    );
    vauchi.create_identity("Alice").unwrap();
    vauchi.advance_onboarding().unwrap();

    // SkipGate → GroupsSetup (continue, don't skip)
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::SkipGate
    );
    vauchi.advance_onboarding().unwrap();

    // GroupsSetup: create some groups
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::GroupsSetup
    );
    let created = vauchi
        .create_suggested_groups(&["Family", "Friends"])
        .unwrap();
    assert_eq!(created.len(), 2, "Should create 2 groups");
    assert_eq!(created[0].name(), "Family");
    assert_eq!(created[1].name(), "Friends");

    // GroupsSetup → ContactInfo
    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::ContactInfo
    );

    // ContactInfo → PreviewCard
    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::PreviewCard
    );

    // Verify card exists (created during identity creation)
    let card = vauchi.own_card().unwrap();
    assert!(
        card.is_some(),
        "Own card should exist after identity creation"
    );
    assert_eq!(
        card.unwrap().display_name(),
        "Alice",
        "Card display name should match identity"
    );

    // PreviewCard → SecurityExplanation
    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::SecurityExplanation
    );

    // SecurityExplanation → BackupPrompt
    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::BackupPrompt
    );

    // BackupPrompt → Ready
    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::Ready
    );

    // Verify completion percentage before final advance
    assert_eq!(
        vauchi.onboarding_completion_percentage().unwrap(),
        90,
        "Should be 90% (10/11 steps completed) before final advance"
    );

    // Final advance: Ready → complete
    vauchi.advance_onboarding().unwrap();
    assert!(
        vauchi.is_onboarding_complete().unwrap(),
        "Onboarding should be complete"
    );
    assert_eq!(
        vauchi.onboarding_completion_percentage().unwrap(),
        100,
        "Should be 100% after completion"
    );

    // Verify all steps completed
    let progress = vauchi.get_onboarding_progress().unwrap();
    assert_eq!(
        progress.completed_steps.len(),
        11,
        "All 11 steps should be completed"
    );
}
