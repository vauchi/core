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
// Atomic identity-creation + onboarding-completion (audit
// `2026-04-28-app-launch-and-identity-orchestration-in-core` §2.5)
// =============================================================================

// @internal
#[test]
fn create_identity_with_onboarding_marks_complete_in_one_call() {
    let mut vauchi = create_test_vauchi();
    assert!(!vauchi.has_identity());
    assert!(!vauchi.is_onboarding_complete().unwrap());

    vauchi.create_identity_with_onboarding("Alice").unwrap();

    assert!(vauchi.has_identity(), "identity must be created");
    assert!(
        vauchi.is_onboarding_complete().unwrap(),
        "onboarding must be marked complete in the same call — \
         no crash window between identity creation and completion flag"
    );
}

// @internal
#[test]
fn create_identity_with_onboarding_rejects_second_call() {
    let mut vauchi = create_test_vauchi();
    vauchi.create_identity_with_onboarding("Alice").unwrap();

    let err = vauchi
        .create_identity_with_onboarding("Bob")
        .expect_err("second create must fail loudly, not overwrite");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("AlreadyInitialized") || msg.contains("Already"),
        "expected AlreadyInitialized error, got {msg}"
    );
}

// @internal
#[test]
fn mark_onboarding_complete_is_idempotent() {
    let vauchi = create_test_vauchi();
    vauchi.mark_onboarding_complete().unwrap();
    assert!(vauchi.is_onboarding_complete().unwrap());
    // Calling again is a no-op.
    vauchi.mark_onboarding_complete().unwrap();
    assert!(vauchi.is_onboarding_complete().unwrap());
}

// @internal
#[test]
fn create_identity_with_onboarding_then_boot_lands_on_main() {
    // Atomic-helper version of the existing
    // "create-identity-then-boot" path: the new helper marks
    // onboarding complete in the same call, so post-boot the user
    // lands on the default screen (MyInfo when no password).
    let mut vauchi = create_test_vauchi();
    vauchi.create_identity_with_onboarding("Alice").unwrap();
    assert!(vauchi.is_onboarding_complete().unwrap());

    let engine = vauchi_app::ui::AppEngine::new(vauchi);
    assert_eq!(
        engine.current_app_screen(),
        &vauchi_app::ui::AppScreen::MyInfo,
        "atomic create_identity_with_onboarding -> identity + \
         onboarding complete -> boot lands on MyInfo"
    );
}

// =============================================================================
// Periodic sync tick (audit
// `2026-04-28-lifecycle-session-residue-umbrella` item P2-C)
// =============================================================================

// @internal
#[cfg(feature = "network-http")]
#[test]
fn periodic_sync_tick_returns_no_identity_when_no_identity() {
    let mut vauchi = create_test_vauchi();
    let outcome = vauchi
        .periodic_sync_tick()
        .expect("tick should not error before identity");
    assert!(
        matches!(outcome, vauchi_core::VauchiSyncOutcome::NoIdentity),
        "expected NoIdentity, got {outcome:?}"
    );
}

// @internal
#[cfg(feature = "network-http")]
#[test]
fn periodic_sync_tick_returns_not_connected_after_identity_no_connect() {
    let mut vauchi = create_test_vauchi();
    vauchi.create_identity_with_onboarding("Alice").unwrap();
    let outcome = vauchi.periodic_sync_tick().expect("tick should not error");
    assert!(
        matches!(outcome, vauchi_core::VauchiSyncOutcome::NotConnected),
        "expected NotConnected, got {outcome:?}"
    );
}

// @internal
#[cfg(feature = "network-http")]
#[test]
fn periodic_sync_constants_match_audit_recommendation() {
    // Audit P2-C calls out a 15-min interval and a 3-retry policy
    // — both currently duplicated as platform-side magic numbers.
    // Locking the values here means a frontend-side change cannot
    // drift silently.
    assert_eq!(vauchi_core::PERIODIC_SYNC_INTERVAL_SECONDS, 900);
    assert_eq!(vauchi_core::PERIODIC_SYNC_MAX_RETRIES, 3);
}

// =============================================================================
// =============================================================================

// @scenario: onboarding:api_advance (#26)
#[test]
fn test_api_get_and_advance_onboarding() {
    let vauchi = create_test_vauchi();

    let progress = vauchi.get_onboarding_progress().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::IdentityCheck);

    let progress = vauchi.advance_onboarding().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::LinkChoice);

    let progress = vauchi.get_onboarding_progress().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::LinkChoice);
}

// @scenario: onboarding:api_skip (#24)
#[test]
fn test_api_skip_onboarding_step() {
    let vauchi = create_test_vauchi();

    let progress = vauchi.skip_onboarding_step().unwrap();
    assert_eq!(progress.current_step(), OnboardingStep::LinkChoice);

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

    vauchi.advance_onboarding().unwrap();
    vauchi.advance_onboarding().unwrap();

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

    for _ in 0..6 {
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
    assert_eq!(vauchi.onboarding_completion_percentage().unwrap(), 16); // 1/6

    for _ in 0..5 {
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

    vauchi.create_suggested_groups(&["Family"]).unwrap();

    // Create again with overlap — should skip Family, create Friends
    let created = vauchi
        .create_suggested_groups(&["Family", "Friends"])
        .unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name(), "Friends");
}

// =============================================================================
// =============================================================================

// @scenario: onboarding:skip_step_at_what_next_completes
#[test]
fn test_skip_step_at_what_next_marks_complete() {
    let mut vauchi = create_test_vauchi();

    // Advance to DefaultName (IdentityCheck -> LinkChoice -> DefaultName)
    vauchi.advance_onboarding().unwrap();
    vauchi.advance_onboarding().unwrap();
    vauchi.create_identity("Alice").unwrap();

    // Advance through all steps to WhatNext
    for _ in 0..3 {
        vauchi.advance_onboarding().unwrap();
    }
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::WhatNext
    );
    assert!(
        !vauchi.is_onboarding_complete().unwrap(),
        "Should not be complete before skip_step at WhatNext"
    );

    // skip_step at WhatNext should complete onboarding (else branch in skip_step)
    vauchi.skip_onboarding_step().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::WhatNext,
        "Should stay at WhatNext"
    );
    assert!(
        vauchi.is_onboarding_complete().unwrap(),
        "skip_step at WhatNext should mark onboarding complete"
    );
}

// =============================================================================
// =============================================================================

// @scenario: onboarding:identity_check_create_new
#[test]
fn test_identity_check_create_new_goes_to_default_name() {
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
            assert_eq!(screen.screen_id, "default_name");
            let progress = screen
                .progress
                .as_ref()
                .expect("DefaultName should have progress");
            assert_eq!(progress.current_step, 1);
            assert_eq!(progress.total_steps, 4);
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
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore_backup".into(),
    });
    use vauchi_core::{Command, FilePickPurpose};
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
        other => panic!("expected Commands(FilePickFromUser/ImportBackup), got {other:?}"),
    }
}

// @scenario: onboarding:link_choice_back
#[test]
fn test_link_choice_back_returns_to_identity_check() {
    use vauchi_app::ui::{ActionResult, OnboardingEngine, UserAction, WorkflowEngine};

    let mut engine = OnboardingEngine::new();
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

// @scenario: onboarding:e2e_full_flow
#[test]
fn test_full_onboarding_flow() {
    let mut vauchi = create_test_vauchi();

    // IdentityCheck → LinkChoice
    vauchi.advance_onboarding().unwrap();

    // LinkChoice → DefaultName
    vauchi.advance_onboarding().unwrap();

    // DefaultName → GroupsSetup (create identity first)
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::DefaultName
    );
    vauchi.create_identity("Alice").unwrap();
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

    // ContactInfo → WhatNext
    vauchi.advance_onboarding().unwrap();
    assert_eq!(
        vauchi.current_onboarding_step().unwrap(),
        OnboardingStep::WhatNext
    );

    assert_eq!(
        vauchi.onboarding_completion_percentage().unwrap(),
        83,
        "Should be 83% (5/6 steps completed) before final advance"
    );

    // Final advance: WhatNext → complete
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

    let progress = vauchi.get_onboarding_progress().unwrap();
    assert_eq!(
        progress.completed_steps.len(),
        6,
        "All 6 steps should be completed"
    );
}
