// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// ── Helpers ──────────────────────────────────────────────────────────

fn advance_to_default_name(engine: &mut OnboardingEngine) {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
}

fn advance_to_groups_setup(engine: &mut OnboardingEngine) {
    advance_to_default_name(engine);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
}

fn advance_to_contact_info(engine: &mut OnboardingEngine) {
    advance_to_groups_setup(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
}

fn advance_to_what_next(engine: &mut OnboardingEngine) {
    advance_to_contact_info(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
}

// ── Pre-gate: IdentityCheck ─────────────────────────────────────────

// @internal
#[test]
fn starts_at_identity_check() {
    let engine = OnboardingEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "identity_check");
    assert!(
        screen.progress.is_none(),
        "Pre-gate screens have no progress bar"
    );
}

// @internal
#[test]
fn identity_check_has_info_panel_and_three_actions() {
    let engine = OnboardingEngine::new();
    let screen = engine.current_screen();

    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InfoPanel { .. })),
        "IdentityCheck should have an InfoPanel"
    );
    assert_eq!(screen.actions.len(), 3);
    assert_eq!(screen.actions[0].id, "create_new");
    assert!(matches!(screen.actions[0].style, ActionStyle::Primary));
    assert_eq!(screen.actions[1].id, "link_device");
    assert!(matches!(screen.actions[1].style, ActionStyle::Secondary));
    assert_eq!(screen.actions[2].id, "load_backup");
    assert!(matches!(screen.actions[2].style, ActionStyle::Secondary));
}

// @internal
#[test]
fn identity_check_create_new_goes_to_default_name() {
    let mut engine = OnboardingEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "default_name");
            assert_eq!(screen.progress.as_ref().unwrap().current_step, 1);
        }
        other => panic!("Expected NavigateTo default_name, got {other:?}"),
    }
}

// @internal
#[test]
fn identity_check_link_device_navigates_to_instructions() {
    let mut engine = OnboardingEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "link_device".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "device_link_instructions");
            assert!(
                screen.progress.is_none(),
                "Pre-gate screens have no progress bar"
            );
            // The instruction screen's scan button emits `QrRequestScan`
            // directly; `StartDeviceLink` is no longer emitted from
            // onboarding (`2026-07-06-mobile-domain-shell-violations` I9).
        }
        other => panic!("Expected NavigateTo(device_link_instructions), got {other:?}"),
    }
}

// @internal
#[test]
fn identity_check_load_backup_emits_file_pick_command() {
    use vauchi_core::{Command, FilePickPurpose};
    let mut engine = OnboardingEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "load_backup".into(),
    });
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

// ── Pre-gate: DeviceLinkInstructions ────────────────────────────────

// @internal
#[test]
fn device_link_instructions_scan_re_emits_qr_request() {
    use vauchi_core::Command;
    let mut engine = OnboardingEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "link_device".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "scan_qr".into(),
    });
    match result {
        ActionResult::Commands { commands } => {
            assert_eq!(commands.len(), 1);
            assert!(
                matches!(commands[0], Command::QrRequestScan),
                "scan_qr should request QR scan"
            );
        }
        other => panic!("Expected Commands, got {other:?}"),
    }
}

// @internal
#[test]
fn device_link_instructions_back_returns_to_identity_check() {
    let mut engine = OnboardingEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "link_device".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "identity_check");
        }
        other => panic!("Expected NavigateTo(identity_check), got {other:?}"),
    }
}

// ── Screen 1: DefaultName ───────────────────────────────────────────

// @internal
#[test]
fn default_name_has_text_input() {
    let mut engine = OnboardingEngine::new();
    advance_to_default_name(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "default_name");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 1);
    assert_eq!(screen.progress.as_ref().unwrap().total_steps, 4);

    let has_input = screen.components.iter().any(|c| {
        matches!(c, Component::TextInput { id, input_type, max_length, ..
        }
            if id == "display_name"
            && *input_type == InputType::Text
            && *max_length == Some(100))
    });
    assert!(
        has_input,
        "DefaultName should have a TextInput for display_name"
    );
}

// @internal
#[test]
fn default_name_validation_rejects_empty() {
    let mut engine = OnboardingEngine::new();
    advance_to_default_name(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "display_name");
            assert!(!message.is_empty());
        }
        other => panic!("Expected ValidationError for empty name, got {other:?}"),
    }
}

// @internal
#[test]
fn default_name_validation_rejects_whitespace_only() {
    let mut engine = OnboardingEngine::new();
    advance_to_default_name(&mut engine);

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "   ".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(result, ActionResult::ValidationError { .. }),
        "Whitespace-only name should be rejected"
    );
}

// @internal
#[test]
fn default_name_text_changed_updates_screen() {
    let mut engine = OnboardingEngine::new();
    advance_to_default_name(&mut engine);

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "default_name");
            let continue_btn = screen.actions.iter().find(|a| a.id == "continue").unwrap();
            assert!(continue_btn.enabled);
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @internal
#[test]
fn default_name_continue_navigates_to_groups_setup() {
    let mut engine = OnboardingEngine::new();
    advance_to_default_name(&mut engine);

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "groups_setup"),
        other => panic!("Expected NavigateTo groups_setup, got {other:?}"),
    }
}

// ── Screen 2: GroupsSetup ───────────────────────────────────────────

// @internal
#[test]
fn groups_setup_has_toggle_list_with_suggested_labels() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "groups_setup");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 2);

    let toggle_list = screen.components.iter().find_map(|c| match c {
        Component::ToggleList { id, items, .. } if id == "groups" => Some(items),
        _ => None,
    });
    let items = toggle_list.expect("Should have a ToggleList with id 'groups'");

    assert_eq!(items.len(), 4);
    assert_eq!(items[0].id, "Family");
    assert_eq!(items[1].id, "Friends");
    assert_eq!(items[2].id, "Coworkers");
    assert_eq!(items[3].id, "Business");
    assert!(
        items.iter().all(|i| !i.selected),
        "All groups start unselected"
    );
}

// @internal
#[test]
fn groups_toggle_selects_and_deselects() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Family".into(),
    });
    match &result {
        ActionResult::UpdateScreen(screen) => {
            let items = screen.components.iter().find_map(|c| match c {
                Component::ToggleList { id, items, .. } if id == "groups" => Some(items),
                _ => None,
            });
            let family = items.unwrap().iter().find(|i| i.id == "Family").unwrap();
            assert!(family.selected, "Family should be selected after toggle");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }

    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Family".into(),
    });
    match &result {
        ActionResult::UpdateScreen(screen) => {
            let items = screen.components.iter().find_map(|c| match c {
                Component::ToggleList { id, items, .. } if id == "groups" => Some(items),
                _ => None,
            });
            let family = items.unwrap().iter().find(|i| i.id == "Family").unwrap();
            assert!(
                !family.selected,
                "Family should be deselected after second toggle"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @internal
#[test]
fn groups_continue_goes_to_contact_info() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "contact_info"),
        other => panic!("Expected NavigateTo contact_info, got {other:?}"),
    }
}

// @internal
#[test]
fn groups_skip_goes_to_contact_info() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "contact_info"),
        other => panic!("Expected NavigateTo contact_info, got {other:?}"),
    }
}

// ── Screen 3: ContactInfo ───────────────────────────────────────────

// @scenario: onboarding :: Quick add phone and email
// @internal
#[test]
fn contact_info_has_quick_add_buttons() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_info");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 3);

    let has_field_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::FieldList { .. }));
    assert!(!has_field_list, "Quick-add should NOT have FieldList");

    let action_ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert!(
        action_ids.contains(&"show_phone"),
        "Should have show_phone action"
    );
    assert!(
        action_ids.contains(&"show_email"),
        "Should have show_email action"
    );
    assert!(
        action_ids.contains(&"add_social"),
        "Should have add_social action"
    );
    assert!(
        action_ids.contains(&"continue"),
        "Should have continue action"
    );
    assert!(action_ids.contains(&"skip"), "Should have skip action");
}

// @scenario: onboarding :: Quick add phone and email
// @internal
#[test]
fn contact_info_show_phone_reveals_input() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_phone".into(),
    });
    let screen = match result {
        ActionResult::UpdateScreen(s) => s,
        other => panic!("Expected UpdateScreen, got {other:?}"),
    };

    let has_phone_input = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::TextInput { id, .. } if id == "phone_input"));
    assert!(has_phone_input, "Phone input should be visible");

    let has_show_phone = screen.actions.iter().any(|a| a.id == "show_phone");
    assert!(!has_show_phone, "show_phone button should be hidden");
}

// @scenario: onboarding :: Quick add phone and email
// @internal
#[test]
fn contact_info_show_email_reveals_input() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_email".into(),
    });
    let screen = match result {
        ActionResult::UpdateScreen(s) => s,
        other => panic!("Expected UpdateScreen, got {other:?}"),
    };

    let has_email_input = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::TextInput { id, .. } if id == "email_input"));
    assert!(has_email_input, "Email input should be visible");
}

// @scenario: onboarding :: Quick add phone and email
// @internal
#[test]
fn contact_info_typing_phone_updates_value() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_phone".into(),
    });

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "phone_input".into(),
        value: "+41 79 123 4567".into(),
    });
    let screen = match result {
        ActionResult::UpdateScreen(s) => s,
        other => panic!("Expected UpdateScreen, got {other:?}"),
    };

    let phone_value = screen.components.iter().find_map(|c| match c {
        Component::TextInput { id, value, .. } if id == "phone_input" => Some(value.clone()),
        _ => None,
    });
    assert_eq!(phone_value, Some("+41 79 123 4567".into()));
}

// @scenario: onboarding :: Quick add phone and email
// @internal
#[test]
fn contact_info_continue_syncs_fields_to_data() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_phone".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "phone_input".into(),
        value: "+41 79 000 0000".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_email".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "email_input".into(),
        value: "alice@example.com".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(ref s) if s.screen_id == "what_next"),
        "Should navigate to what_next"
    );

    let data = engine.data();
    let phone = data.fields.iter().find(|f| f.field_type == "phone");
    let email = data.fields.iter().find(|f| f.field_type == "email");
    assert_eq!(phone.map(|f| f.value.as_str()), Some("+41 79 000 0000"));
    assert_eq!(email.map(|f| f.value.as_str()), Some("alice@example.com"));
    assert!(phone.unwrap().shown);
    assert!(email.unwrap().shown);
}

// @scenario: onboarding :: Quick add phone and email
// @internal
#[test]
fn contact_info_skip_does_not_sync_empty_fields() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_phone".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(ref s) if s.screen_id == "what_next"));
    assert!(
        engine.data().fields.is_empty(),
        "Skip should not add empty fields"
    );
}

// @internal
#[test]
fn contact_info_add_social_falls_through() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_social".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "Engine returns UpdateScreen; AppEngine intercepts add_social"
    );
}

// @internal
#[test]
fn contact_info_continue_goes_to_what_next() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "what_next"),
        other => panic!("Expected NavigateTo what_next, got {other:?}"),
    }
}

// ── Screen 4: WhatNext ─────────────────────────────────────────────

/// Final onboarding step: one filled primary ("Start using the app"
/// — the natural finish line) plus two outlined shortcuts (Exchange,
/// Import). "Read about security" / "Read about backup" came off
/// this screen 2026-05-21 (problem record
/// `2026-05-21-mobile-onboarding-final-step-and-skip-fold` G2/G3):
/// the docs belong in Help, not as peers of the finish line.
#[test]
fn what_next_has_three_actions_with_start_app_as_primary() {
    let mut engine = OnboardingEngine::new();
    advance_to_what_next(&mut engine);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "what_next");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 4);
    assert_eq!(screen.progress.as_ref().unwrap().total_steps, 4);
    assert_eq!(screen.actions.len(), 3);
    assert_eq!(screen.actions[0].id, "start_app");
    assert!(matches!(screen.actions[0].style, ActionStyle::Primary));
    assert_eq!(screen.actions[1].id, "exchange");
    assert!(matches!(screen.actions[1].style, ActionStyle::Secondary));
    assert_eq!(screen.actions[2].id, "import_contacts");
    assert!(matches!(screen.actions[2].style, ActionStyle::Secondary));
}

#[test]
fn what_next_exchange_emits_onboarding_complete_with_exchange() {
    let mut engine = OnboardingEngine::new();
    advance_to_what_next(&mut engine);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "exchange".into(),
    });
    assert!(matches!(
        result,
        ActionResult::OnboardingComplete {
            destination: PostOnboardingDestination::Exchange
        }
    ));
}

#[test]
fn what_next_start_app_emits_onboarding_complete_with_main_screen() {
    let mut engine = OnboardingEngine::new();
    advance_to_what_next(&mut engine);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_app".into(),
    });
    assert!(matches!(
        result,
        ActionResult::OnboardingComplete {
            destination: PostOnboardingDestination::MainScreen
        }
    ));
}

#[test]
fn what_next_import_contacts_emits_onboarding_complete_with_import() {
    let mut engine = OnboardingEngine::new();
    advance_to_what_next(&mut engine);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "import_contacts".into(),
    });
    assert!(matches!(
        result,
        ActionResult::OnboardingComplete {
            destination: PostOnboardingDestination::ImportContacts
        }
    ));
}

/// "Read about security" / "Read about backup" are no longer surfaced
/// from the final onboarding step (G3 of
/// `2026-05-21-mobile-onboarding-final-step-and-skip-fold`). The
/// handler no longer recognises those action_ids — pressing one is
/// indistinguishable from any other unknown action and returns an
/// `UpdateScreen`, leaving the user on `what_next`. Guards against a
/// regression that adds them back without restoring the buttons.
#[test]
fn what_next_legacy_docs_action_ids_no_longer_complete() {
    for legacy_id in ["read_security", "read_backup"] {
        let mut engine = OnboardingEngine::new();
        advance_to_what_next(&mut engine);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: legacy_id.into(),
        });
        match result {
            ActionResult::UpdateScreen(screen) => {
                assert_eq!(
                    screen.screen_id, "what_next",
                    "{legacy_id} must keep the user on what_next"
                );
            }
            other => panic!(
                "{legacy_id} must no longer route to a destination — \
                 got {other:?}"
            ),
        }
    }
}

// ── Full flow ───────────────────────────────────────────────────────

// @internal
#[test]
fn full_flow_to_completion() {
    let mut engine = OnboardingEngine::new();

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
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
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_app".into(),
    });
    assert!(matches!(
        result,
        ActionResult::OnboardingComplete {
            destination: PostOnboardingDestination::MainScreen
        }
    ));
}

// ── Data accessor ───────────────────────────────────────────────────

// @internal
#[test]
fn data_accessor_returns_collected_data() {
    let mut engine = OnboardingEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });

    assert_eq!(engine.data().display_name, "Alice");
}

// @internal
#[test]
fn data_reflects_selected_groups() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Family".into(),
    });
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Friends".into(),
    });

    let selected: Vec<&str> = engine
        .data()
        .selected_groups
        .iter()
        .filter(|g| g.selected)
        .map(|g| g.name.as_str())
        .collect();
    assert_eq!(selected, vec!["Family", "Friends"]);
}

// ── A11y labels ────────────────────────────────────────────────────

// @scenario: accessibility :: onboarding text inputs have a11y labels
// @internal
#[test]
fn onboarding_text_input_has_a11y_label() {
    let mut engine = OnboardingEngine::new();
    advance_to_default_name(&mut engine);

    let screen = engine.current_screen();
    let text_input = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::TextInput { id, .. } if id == "display_name"));
    assert!(text_input.is_some(), "display_name TextInput not found");
    match text_input.unwrap() {
        Component::TextInput { a11y, .. } => {
            let a11y = a11y.as_ref().expect("a11y must be populated");
            assert_eq!(a11y.label.as_deref(), Some("Display name input"));
            assert!(a11y.hint.is_some(), "hint should describe what to enter");
        }
        _ => unreachable!(),
    }
}

// @scenario: accessibility :: onboarding custom group input has a11y label
// @internal
#[test]
fn onboarding_custom_group_text_input_has_a11y_label() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    let screen = engine.current_screen();
    let text_input = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::TextInput { id, .. } if id == "custom_group"));
    assert!(text_input.is_some(), "custom_group TextInput not found");
    match text_input.unwrap() {
        Component::TextInput { a11y, .. } => {
            let a11y = a11y.as_ref().expect("a11y must be populated");
            assert_eq!(a11y.label.as_deref(), Some("Custom group name input"));
            assert!(a11y.hint.is_some(), "hint should describe what to enter");
        }
        _ => unreachable!(),
    }
}

// ── Subtitles ──────────────────────────────────────────────────────

// @internal
#[test]
fn default_name_step_has_expected_subtitle() {
    let mut engine = OnboardingEngine::new();
    advance_to_default_name(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "default_name");
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("How you appear to contacts")
    );
}

// @internal
#[test]
fn groups_step_has_expected_subtitle() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "groups_setup");
    assert_eq!(screen.subtitle.as_deref(), Some("Organize who sees what"));
}

// @internal
#[test]
fn contact_info_step_has_subtitle() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_info");
    assert!(
        screen.subtitle.is_some(),
        "contact_info step must have a subtitle"
    );
}

// @internal
#[test]
fn what_next_step_has_expected_subtitle() {
    let mut engine = OnboardingEngine::new();
    advance_to_what_next(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "what_next");
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("This is what contacts will see")
    );
}

// ── info_key / help icons ──────────────────────────────────────────

// @internal
#[test]
fn groups_toggle_items_have_info_key_when_help_enabled() {
    let mut engine = OnboardingEngine::new().with_help_icons(true);
    advance_to_groups_setup(&mut engine);

    let screen = engine.current_screen();
    let items = screen.components.iter().find_map(|c| match c {
        Component::ToggleList { id, items, .. } if id == "groups" => Some(items),
        _ => None,
    });
    let items = items.expect("groups ToggleList not found");
    assert!(!items.is_empty(), "must have at least one toggle item");
    for item in items {
        assert_eq!(
            item.info_key.as_deref(),
            Some("groups_purpose"),
            "item '{}' should have info_key when help is enabled",
            item.id
        );
    }
}

// @internal
#[test]
fn groups_toggle_items_have_no_info_key_when_help_disabled() {
    let mut engine = OnboardingEngine::new(); // help off by default
    advance_to_groups_setup(&mut engine);

    let screen = engine.current_screen();
    let items = screen.components.iter().find_map(|c| match c {
        Component::ToggleList { id, items, .. } if id == "groups" => Some(items),
        _ => None,
    });
    let items = items.expect("groups ToggleList not found");
    assert!(!items.is_empty(), "must have at least one toggle item");
    for item in items {
        assert_eq!(
            item.info_key, None,
            "item '{}' should have no info_key when help is disabled",
            item.id
        );
    }
}

// @internal
#[test]
fn contact_info_phone_input_has_info_key_when_help_enabled() {
    let mut engine = OnboardingEngine::new().with_help_icons(true);
    advance_to_contact_info(&mut engine);

    // Reveal the phone input first
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_phone".into(),
    });

    let screen = engine.current_screen();
    let phone_input = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::TextInput { id, .. } if id == "phone_input"));
    assert!(phone_input.is_some(), "phone_input not found");
    match phone_input.unwrap() {
        Component::TextInput { info_key, .. } => {
            assert_eq!(
                info_key.as_deref(),
                Some("contact_info_optional"),
                "phone_input should have info_key when help is enabled"
            );
        }
        _ => unreachable!(),
    }
}

// @internal
#[test]
fn contact_info_phone_input_has_no_info_key_when_help_disabled() {
    let mut engine = OnboardingEngine::new(); // help off by default
    advance_to_contact_info(&mut engine);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_phone".into(),
    });

    let screen = engine.current_screen();
    let phone_input = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::TextInput { id, .. } if id == "phone_input"));
    assert!(phone_input.is_some(), "phone_input not found");
    match phone_input.unwrap() {
        Component::TextInput { info_key, .. } => {
            assert_eq!(
                info_key, &None,
                "phone_input should have no info_key when help is disabled"
            );
        }
        _ => unreachable!(),
    }
}

// @internal
#[test]
fn contact_info_email_input_has_info_key_when_help_enabled() {
    let mut engine = OnboardingEngine::new().with_help_icons(true);
    advance_to_contact_info(&mut engine);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_email".into(),
    });

    let screen = engine.current_screen();
    let email_input = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::TextInput { id, .. } if id == "email_input"));
    assert!(email_input.is_some(), "email_input not found");
    match email_input.unwrap() {
        Component::TextInput { info_key, .. } => {
            assert_eq!(
                info_key.as_deref(),
                Some("contact_info_optional"),
                "email_input should have info_key when help is enabled"
            );
        }
        _ => unreachable!(),
    }
}

// @internal
#[test]
fn contact_info_email_input_has_no_info_key_when_help_disabled() {
    let mut engine = OnboardingEngine::new(); // help off by default
    advance_to_contact_info(&mut engine);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "show_email".into(),
    });

    let screen = engine.current_screen();
    let email_input = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::TextInput { id, .. } if id == "email_input"));
    assert!(email_input.is_some(), "email_input not found");
    match email_input.unwrap() {
        Component::TextInput { info_key, .. } => {
            assert_eq!(
                info_key, &None,
                "email_input should have no info_key when help is disabled"
            );
        }
        _ => unreachable!(),
    }
}
