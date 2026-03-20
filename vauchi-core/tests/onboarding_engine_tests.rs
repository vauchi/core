// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

// ── Helpers ──────────────────────────────────────────────────────────

fn advance_to_welcome(engine: &mut OnboardingEngine) {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
}

fn advance_to_default_name(engine: &mut OnboardingEngine) {
    advance_to_welcome(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
}

fn advance_to_skip_gate(engine: &mut OnboardingEngine) {
    advance_to_default_name(engine);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
}

fn advance_to_groups_setup(engine: &mut OnboardingEngine) {
    advance_to_skip_gate(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue_setup".into(),
    });
}

fn advance_to_contact_info(engine: &mut OnboardingEngine) {
    advance_to_groups_setup(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
}

fn advance_to_preview_card(engine: &mut OnboardingEngine) {
    advance_to_contact_info(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
}

fn advance_to_security_explanation(engine: &mut OnboardingEngine) {
    advance_to_preview_card(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
}

fn advance_to_backup_prompt(engine: &mut OnboardingEngine) {
    advance_to_security_explanation(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
}

fn advance_to_ready(engine: &mut OnboardingEngine) {
    advance_to_backup_prompt(engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
}

// ── Pre-gate: IdentityCheck ─────────────────────────────────────────

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

#[test]
fn identity_check_has_info_panel_and_two_actions() {
    let engine = OnboardingEngine::new();
    let screen = engine.current_screen();

    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InfoPanel { .. })),
        "IdentityCheck should have an InfoPanel"
    );
    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "create_new");
    assert!(matches!(screen.actions[0].style, ActionStyle::Primary));
    assert_eq!(screen.actions[1].id, "have_identity");
    assert!(matches!(screen.actions[1].style, ActionStyle::Secondary));
}

#[test]
fn identity_check_create_new_goes_to_welcome() {
    let mut engine = OnboardingEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "welcome");
            assert_eq!(screen.progress.as_ref().unwrap().current_step, 1);
        }
        other => panic!("Expected NavigateTo welcome, got {other:?}"),
    }
}

#[test]
fn identity_check_have_identity_goes_to_link_choice() {
    let mut engine = OnboardingEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "link_choice");
            assert!(
                screen.progress.is_none(),
                "Pre-gate screens have no progress bar"
            );
        }
        other => panic!("Expected NavigateTo link_choice, got {other:?}"),
    }
}

// ── Pre-gate: LinkChoice ────────────────────────────────────────────

#[test]
fn link_choice_link_device_returns_start_device_link() {
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

#[test]
fn link_choice_restore_backup_returns_start_backup_import() {
    let mut engine = OnboardingEngine::new();
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

#[test]
fn link_choice_back_returns_to_identity_check() {
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
        other => panic!("Expected NavigateTo identity_check, got {other:?}"),
    }
}

#[test]
fn identity_check_unknown_action_returns_update_screen() {
    let mut engine = OnboardingEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "identity_check");
        }
        other => panic!("Expected UpdateScreen for unknown action, got {other:?}"),
    }
}

// ── Screen 1: Welcome ───────────────────────────────────────────────

#[test]
fn welcome_has_info_panel_and_one_action() {
    let mut engine = OnboardingEngine::new();
    advance_to_welcome(&mut engine);
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "welcome");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 1);
    assert_eq!(screen.progress.as_ref().unwrap().total_steps, 9);
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InfoPanel { .. })),
        "Welcome screen should have an InfoPanel"
    );
    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, "get_started");
    assert!(matches!(screen.actions[0].style, ActionStyle::Primary));
}

#[test]
fn welcome_to_default_name() {
    let mut engine = OnboardingEngine::new();
    advance_to_welcome(&mut engine);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "default_name");
            assert_eq!(screen.progress.as_ref().unwrap().current_step, 2);
        }
        other => panic!("Expected NavigateTo, got {other:?}"),
    }
}

// ── Screen 2: DefaultName ───────────────────────────────────────────

#[test]
fn default_name_has_text_input() {
    let mut engine = OnboardingEngine::new();
    advance_to_default_name(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "default_name");

    let has_input = screen.components.iter().any(|c| {
        matches!(c, Component::TextInput { id, input_type, max_length, .. }
            if id == "display_name"
            && *input_type == InputType::Text
            && *max_length == Some(100))
    });
    assert!(
        has_input,
        "DefaultName should have a TextInput for display_name"
    );
}

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
            // Continue button should now be enabled
            let continue_btn = screen.actions.iter().find(|a| a.id == "continue").unwrap();
            assert!(continue_btn.enabled);
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn default_name_continue_navigates_to_skip_gate() {
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
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "skip_gate"),
        other => panic!("Expected NavigateTo skip_gate, got {other:?}"),
    }
}

// ── Screen 3: SkipGate ──────────────────────────────────────────────

#[test]
fn skip_gate_has_correct_actions() {
    let mut engine = OnboardingEngine::new();
    advance_to_skip_gate(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "skip_gate");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 3);
    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "continue_setup");
    assert_eq!(screen.actions[1].id, "skip_to_finish");
}

#[test]
fn skip_gate_continue_setup_goes_to_groups() {
    let mut engine = OnboardingEngine::new();
    advance_to_skip_gate(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue_setup".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "groups_setup"),
        other => panic!("Expected NavigateTo groups_setup, got {other:?}"),
    }
}

#[test]
fn skip_gate_skip_to_finish_goes_to_security() {
    let mut engine = OnboardingEngine::new();
    advance_to_skip_gate(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip_to_finish".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "security_explanation"),
        other => panic!("Expected NavigateTo security_explanation, got {other:?}"),
    }
}

// ── Screen 4: GroupsSetup ───────────────────────────────────────────

#[test]
fn groups_setup_has_toggle_list_with_suggested_labels() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "groups_setup");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 4);

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

#[test]
fn groups_toggle_selects_and_deselects() {
    let mut engine = OnboardingEngine::new();
    advance_to_groups_setup(&mut engine);

    // Toggle Family on
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

    // Toggle Family off
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

// ── Screen 5: ContactInfo ───────────────────────────────────────────

#[test]
fn contact_info_has_field_list() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_info");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 5);

    let has_field_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::FieldList { id, .. } if id == "fields"));
    assert!(has_field_list, "ContactInfo should have a FieldList");
}

#[test]
fn contact_info_visibility_mode_depends_on_groups() {
    // Without groups selected
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let screen = engine.current_screen();
    let mode = screen.components.iter().find_map(|c| match c {
        Component::FieldList {
            visibility_mode, ..
        } => Some(visibility_mode.clone()),
        _ => None,
    });
    assert_eq!(mode, Some(VisibilityMode::ShowHide));

    // With groups selected
    let mut engine = OnboardingEngine::new();
    advance_to_skip_gate(&mut engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue_setup".into(),
    });
    // Select Family
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Family".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    let mode = screen.components.iter().find_map(|c| match c {
        Component::FieldList {
            visibility_mode, ..
        } => Some(visibility_mode.clone()),
        _ => None,
    });
    assert_eq!(mode, Some(VisibilityMode::PerGroup));
}

#[test]
fn contact_info_available_groups_from_selected() {
    let mut engine = OnboardingEngine::new();
    advance_to_skip_gate(&mut engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue_setup".into(),
    });
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Family".into(),
    });
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Friends".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    let groups = screen.components.iter().find_map(|c| match c {
        Component::FieldList {
            available_groups, ..
        } => Some(available_groups.clone()),
        _ => None,
    });
    assert_eq!(
        groups,
        Some(vec!["Family".to_string(), "Friends".to_string()])
    );
}

#[test]
fn contact_info_continue_goes_to_preview() {
    let mut engine = OnboardingEngine::new();
    advance_to_contact_info(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "preview_card"),
        other => panic!("Expected NavigateTo preview_card, got {other:?}"),
    }
}

// ── Screen 6: PreviewCard ───────────────────────────────────────────

#[test]
fn preview_card_shows_display_name() {
    let mut engine = OnboardingEngine::new();
    advance_to_preview_card(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "preview_card");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 6);

    let name = screen.components.iter().find_map(|c| match c {
        Component::CardPreview { name, .. } => Some(name.clone()),
        _ => None,
    });
    assert_eq!(name, Some("Alice".to_string()));
}

#[test]
fn preview_card_edit_goes_back_to_contact_info() {
    let mut engine = OnboardingEngine::new();
    advance_to_preview_card(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "contact_info"),
        other => panic!("Expected NavigateTo contact_info, got {other:?}"),
    }
}

#[test]
fn preview_card_continue_goes_to_security() {
    let mut engine = OnboardingEngine::new();
    advance_to_preview_card(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "security_explanation"),
        other => panic!("Expected NavigateTo security_explanation, got {other:?}"),
    }
}

// ── Screen 7: SecurityExplanation ───────────────────────────────────

#[test]
fn security_explanation_has_info_panel() {
    let mut engine = OnboardingEngine::new();
    advance_to_security_explanation(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "security_explanation");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 7);
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InfoPanel { .. }))
    );
    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, "continue");
}

#[test]
fn security_explanation_continue_goes_to_backup() {
    let mut engine = OnboardingEngine::new();
    advance_to_security_explanation(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "backup_prompt"),
        other => panic!("Expected NavigateTo backup_prompt, got {other:?}"),
    }
}

// ── Screen 8: BackupPrompt ──────────────────────────────────────────

#[test]
fn backup_prompt_has_two_actions() {
    let mut engine = OnboardingEngine::new();
    advance_to_backup_prompt(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_prompt");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 8);
    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "setup_backup");
    assert_eq!(screen.actions[1].id, "skip");
}

#[test]
fn backup_prompt_setup_goes_to_ready() {
    let mut engine = OnboardingEngine::new();
    advance_to_backup_prompt(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "setup_backup".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "ready"),
        other => panic!("Expected NavigateTo ready, got {other:?}"),
    }
}

#[test]
fn backup_prompt_skip_goes_to_ready() {
    let mut engine = OnboardingEngine::new();
    advance_to_backup_prompt(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "ready"),
        other => panic!("Expected NavigateTo ready, got {other:?}"),
    }
}

// ── Screen 9: Ready ─────────────────────────────────────────────────

#[test]
fn ready_screen_has_start_action() {
    let mut engine = OnboardingEngine::new();
    advance_to_ready(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "ready");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 9);
    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, "start");
}

#[test]
fn ready_start_completes() {
    let mut engine = OnboardingEngine::new();
    advance_to_ready(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "Expected Complete"
    );
}

// ── Full flow ───────────────────────────────────────────────────────

#[test]
fn full_flow_to_completion() {
    let mut engine = OnboardingEngine::new();

    // IdentityCheck -> Welcome
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    // Welcome -> DefaultName
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    // Enter name
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    // DefaultName -> SkipGate
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // SkipGate -> GroupsSetup
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue_setup".into(),
    });
    // GroupsSetup -> ContactInfo
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // ContactInfo -> PreviewCard
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // PreviewCard -> SecurityExplanation
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // SecurityExplanation -> BackupPrompt
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // BackupPrompt -> Ready
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
    // Ready -> Complete
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start".into(),
    });
    assert!(matches!(result, ActionResult::Complete));
}

#[test]
fn skip_flow_bypasses_groups_and_fields() {
    let mut engine = OnboardingEngine::new();

    // IdentityCheck -> Welcome
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    // Welcome -> DefaultName
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Bob".into(),
    });
    // DefaultName -> SkipGate
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // SkipGate -> SecurityExplanation (skip)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip_to_finish".into(),
    });
    match &result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "security_explanation"),
        other => panic!("Expected NavigateTo security_explanation, got {other:?}"),
    }

    // SecurityExplanation -> BackupPrompt
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // BackupPrompt -> Ready
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
    // Ready -> Complete
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start".into(),
    });
    assert!(matches!(result, ActionResult::Complete));
}

// ── Data accessor ───────────────────────────────────────────────────

#[test]
fn data_accessor_returns_collected_data() {
    let mut engine = OnboardingEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });

    assert_eq!(engine.data().display_name, "Alice");
}

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

// ── GroupViewSelected on PreviewCard ─────────────────────────────────

#[test]
fn preview_card_group_view_selected() {
    let mut engine = OnboardingEngine::new();
    advance_to_skip_gate(&mut engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue_setup".into(),
    });
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Family".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Now at PreviewCard
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "preview_card");

    let result = engine.handle_action(UserAction::GroupViewSelected {
        group_name: Some("Family".into()),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let selected = screen.components.iter().find_map(|c| match c {
                Component::CardPreview { selected_group, .. } => Some(selected_group.clone()),
                _ => None,
            });
            assert_eq!(selected, Some(Some("Family".to_string())));
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}
