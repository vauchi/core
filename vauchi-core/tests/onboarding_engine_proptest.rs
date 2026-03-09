// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based tests for the OnboardingEngine state machine.

use proptest::prelude::*;
use vauchi_core::ui::*;

// ── Constants ───────────────────────────────────────────────────────

const ALL_SCREEN_IDS: &[&str] = &[
    "identity_check",
    "link_choice",
    "welcome",
    "default_name",
    "skip_gate",
    "groups_setup",
    "contact_info",
    "preview_card",
    "security_explanation",
    "backup_prompt",
    "ready",
];

const GROUP_NAMES: &[&str] = &["Family", "Friends", "Coworkers", "Business"];

// ── Strategies ──────────────────────────────────────────────────────

fn arb_action_id() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("have_identity".to_string()),
        Just("create_new".to_string()),
        Just("link_device".to_string()),
        Just("restore_backup".to_string()),
        Just("back".to_string()),
        Just("get_started".to_string()),
        Just("continue".to_string()),
        Just("continue_setup".to_string()),
        Just("skip_to_finish".to_string()),
        Just("skip".to_string()),
        Just("edit".to_string()),
        Just("setup_backup".to_string()),
        Just("start".to_string()),
        Just("nonexistent_action".to_string()),
        "[a-z_]{1,20}",
    ]
}

fn arb_component_id() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("display_name".to_string()),
        Just("groups".to_string()),
        Just("fields".to_string()),
        Just("group_name_override_Family".to_string()),
        Just("group_name_override_Friends".to_string()),
        Just("unknown_component".to_string()),
        "field_[0-9]{1,2}",
    ]
}

fn arb_group_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("Family".to_string()),
        Just("Friends".to_string()),
        Just("Coworkers".to_string()),
        Just("Business".to_string()),
        Just("NonexistentGroup".to_string()),
    ]
}

fn arb_text_value() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("   ".to_string()),
        Just("Alice".to_string()),
        "\\PC{1,50}",
    ]
}

fn arb_user_action() -> impl Strategy<Value = UserAction> {
    prop_oneof![
        arb_action_id().prop_map(|action_id| UserAction::ActionPressed { action_id }),
        (arb_component_id(), arb_text_value()).prop_map(|(component_id, value)| {
            UserAction::TextChanged {
                component_id,
                value,
            }
        }),
        (arb_component_id(), arb_group_name()).prop_map(|(component_id, item_id)| {
            UserAction::ItemToggled {
                component_id,
                item_id,
            }
        }),
        (
            "field_[0-9]{1,2}".prop_map(Some),
            prop::option::of(arb_group_name()),
            any::<bool>(),
        )
            .prop_map(|(field_id, group_id, visible)| {
                UserAction::FieldVisibilityChanged {
                    field_id: field_id.unwrap(),
                    group_id,
                    visible,
                }
            }),
        prop::option::of(arb_group_name())
            .prop_map(|group_name| UserAction::GroupViewSelected { group_name }),
    ]
}

/// Sequence of "continue/advance" actions that take the engine from
/// IdentityCheck all the way to Ready (full path, no skipping).
fn advance_to_ready(engine: &mut OnboardingEngine, name: &str) {
    // IdentityCheck → Welcome
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    // Welcome → DefaultName
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: name.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue_setup".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
}

// ── Property 1: Forward progress ────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any valid display name eventually reaches Complete via the full path.
    #[test]
    fn forward_progress_reaches_complete(name in "[A-Za-z ]{1,50}") {
        let trimmed = name.trim().to_string();
        prop_assume!(!trimmed.is_empty());

        let mut engine = OnboardingEngine::new();
        advance_to_ready(&mut engine, &name);

        let screen = engine.current_screen();
        prop_assert_eq!(screen.screen_id, "ready");

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "start".into(),
        });
        prop_assert!(matches!(result, ActionResult::Complete));
    }
}

// ── Property 2: Screen stability ────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Calling `current_screen()` twice yields the same screen_id.
    #[test]
    fn screen_stability(actions in prop::collection::vec(arb_user_action(), 0..30)) {
        let mut engine = OnboardingEngine::new();

        // Navigate past IdentityCheck to Welcome, then to DefaultName
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "create_new".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "get_started".into(),
        });
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "display_name".into(),
            value: "Test".into(),
        });

        for action in actions {
            let _ = engine.handle_action(action);
        }

        let s1 = engine.current_screen();
        let s2 = engine.current_screen();
        prop_assert_eq!(s1.screen_id, s2.screen_id);
        prop_assert_eq!(s1.progress.as_ref().map(|p| p.current_step),
                        s2.progress.as_ref().map(|p| p.current_step));
    }
}

// ── Property 3: Unknown action_id returns UpdateScreen ──────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Pressing a non-existent action_id never panics and returns
    /// UpdateScreen (not NavigateTo, not Complete).
    #[test]
    fn unknown_action_returns_update_screen(
        bogus_id in "[a-z]{5,15}",
        pre_actions in prop::collection::vec(arb_user_action(), 0..10),
    ) {
        // Filter out IDs that happen to be real action IDs
        let real_ids = [
            "have_identity", "create_new", "link_device", "restore_backup",
            "back", "get_started", "continue", "continue_setup",
            "skip_to_finish", "skip", "edit", "setup_backup", "start",
        ];
        prop_assume!(!real_ids.contains(&bogus_id.as_str()));

        let mut engine = OnboardingEngine::new();
        // Navigate past IdentityCheck to Welcome, then set name
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "create_new".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "get_started".into(),
        });
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "display_name".into(),
            value: "Test".into(),
        });

        for action in pre_actions {
            let _ = engine.handle_action(action);
        }

        let screen_before = engine.current_screen().screen_id.clone();
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: bogus_id,
        });

        match result {
            ActionResult::UpdateScreen(screen) => {
                prop_assert_eq!(screen.screen_id, screen_before);
            }
            other => prop_assert!(
                false,
                "Expected UpdateScreen for bogus action, got {:?}",
                other,
            ),
        }
    }
}

// ── Property 4: Name validation ─────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Empty or whitespace-only display names are rejected with
    /// ValidationError when pressing continue on DefaultName.
    #[test]
    fn empty_name_rejected(name in "([ \\t]*){0,20}") {
        prop_assume!(name.trim().is_empty());

        let mut engine = OnboardingEngine::new();
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "create_new".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "get_started".into(),
        });
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "display_name".into(),
            value: name,
        });
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        match result {
            ActionResult::ValidationError { component_id, message } => {
                prop_assert_eq!(component_id, "display_name");
                prop_assert!(!message.is_empty());
            }
            other => prop_assert!(
                false,
                "Expected ValidationError for empty/whitespace name, got {:?}",
                other,
            ),
        }
    }

    /// Non-empty, non-whitespace names pass validation.
    #[test]
    fn non_empty_name_accepted(name in "[A-Za-z][A-Za-z ]{0,49}") {
        prop_assume!(!name.trim().is_empty());

        let mut engine = OnboardingEngine::new();
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "create_new".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "get_started".into(),
        });
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "display_name".into(),
            value: name,
        });
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        prop_assert!(
            matches!(result, ActionResult::NavigateTo(_)),
            "Expected NavigateTo for valid name, got {:?}",
            result,
        );
    }
}

// ── Property 5: Toggle consistency ──────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Toggling a group twice returns it to its original selected state.
    #[test]
    fn toggle_twice_returns_to_original(group_idx in 0..4usize) {
        let group_name = GROUP_NAMES[group_idx];
        let mut engine = OnboardingEngine::new();

        // Navigate to groups_setup
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "create_new".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "get_started".into(),
        });
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "display_name".into(),
            value: "Test".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue_setup".into(),
        });

        // Capture original state
        let original = engine.data()
            .selected_groups
            .iter()
            .find(|g| g.name == group_name)
            .unwrap()
            .selected;

        // Toggle once
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "groups".into(),
            item_id: group_name.into(),
        });
        let after_first = engine.data()
            .selected_groups
            .iter()
            .find(|g| g.name == group_name)
            .unwrap()
            .selected;
        prop_assert_ne!(after_first, original);

        // Toggle again
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "groups".into(),
            item_id: group_name.into(),
        });
        let after_second = engine.data()
            .selected_groups
            .iter()
            .find(|g| g.name == group_name)
            .unwrap()
            .selected;
        prop_assert_eq!(after_second, original);
    }

    /// Toggling a group N times: even N → original, odd N → flipped.
    #[test]
    fn toggle_n_times_parity(
        group_idx in 0..4usize,
        n in 1..20usize,
    ) {
        let group_name = GROUP_NAMES[group_idx];
        let mut engine = OnboardingEngine::new();

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "create_new".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "get_started".into(),
        });
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "display_name".into(),
            value: "Test".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue_setup".into(),
        });

        let original = engine.data()
            .selected_groups
            .iter()
            .find(|g| g.name == group_name)
            .unwrap()
            .selected;

        for _ in 0..n {
            let _ = engine.handle_action(UserAction::ItemToggled {
                component_id: "groups".into(),
                item_id: group_name.into(),
            });
        }

        let final_state = engine.data()
            .selected_groups
            .iter()
            .find(|g| g.name == group_name)
            .unwrap()
            .selected;

        if n % 2 == 0 {
            prop_assert_eq!(final_state, original);
        } else {
            prop_assert_ne!(final_state, original);
        }
    }
}

// ── Property 6: Random action sequences never panic ─────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// No sequence of random UserActions causes a panic. The result is
    /// always one of the four ActionResult variants, and the engine
    /// always reports a valid screen_id.
    #[test]
    fn random_actions_never_panic(actions in prop::collection::vec(arb_user_action(), 0..50)) {
        let mut engine = OnboardingEngine::new();

        for action in actions {
            let result = engine.handle_action(action);
            match &result {
                ActionResult::UpdateScreen(screen) => {
                    prop_assert!(
                        ALL_SCREEN_IDS.contains(&screen.screen_id.as_str()),
                        "Unknown screen_id: {}",
                        screen.screen_id,
                    );
                }
                ActionResult::NavigateTo(screen) => {
                    prop_assert!(
                        ALL_SCREEN_IDS.contains(&screen.screen_id.as_str()),
                        "Unknown screen_id on navigate: {}",
                        screen.screen_id,
                    );
                }
                ActionResult::ValidationError { component_id, message } => {
                    prop_assert!(!component_id.is_empty());
                    prop_assert!(!message.is_empty());
                }
                ActionResult::Complete => {
                    // Complete is valid — engine reached the end
                }
                ActionResult::StartDeviceLink | ActionResult::StartBackupImport => {
                    // External handoff — valid from LinkChoice step
                }
                ActionResult::OpenContact { .. }
                | ActionResult::EditContact { .. }
                | ActionResult::OpenUrl { .. }
                | ActionResult::ShowAlert { .. }
                | ActionResult::RequestCamera
                | ActionResult::WipeComplete => {
                    // Valid external navigation/action results
                }
            }
        }

        // After all actions, current_screen must still return a valid screen
        let screen = engine.current_screen();
        prop_assert!(
            ALL_SCREEN_IDS.contains(&screen.screen_id.as_str()),
            "Unknown screen_id after sequence: {}",
            screen.screen_id,
        );
    }
}

// ── Property: Progress invariants ───────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Every screen always has progress with total_steps == 9 and
    /// current_step in 1..=9.
    #[test]
    fn progress_invariants(actions in prop::collection::vec(arb_user_action(), 0..30)) {
        let mut engine = OnboardingEngine::new();

        for action in actions {
            let _ = engine.handle_action(action);
        }

        let screen = engine.current_screen();
        // Pre-gate screens (identity_check, link_choice) have no progress bar
        if screen.screen_id == "identity_check" || screen.screen_id == "link_choice" {
            prop_assert!(screen.progress.is_none(),
                "Pre-gate screen {} should have no progress", screen.screen_id);
        } else {
            let progress = screen.progress.as_ref().expect("main screens must have progress");
            prop_assert_eq!(progress.total_steps, 9);
            prop_assert!(progress.current_step >= 1 && progress.current_step <= 9,
                "current_step {} out of range", progress.current_step);
        }
    }
}
