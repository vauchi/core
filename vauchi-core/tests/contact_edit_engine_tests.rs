// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// ── Test helpers ────────────────────────────────────────────────────

fn sample_contact() -> EditableContact {
    EditableContact {
        display_name: "Alice".into(),
        fields: vec![
            EditableField {
                id: "phone1".into(),
                field_type: "phone".into(),
                label: "Phone".into(),
                value: "+41 79 123 45 67".into(),
                visible_to_groups: vec!["Family".into()],
                shown: true,
            },
            EditableField {
                id: "email1".into(),
                field_type: "email".into(),
                label: "Email".into(),
                value: "alice@example.com".into(),
                visible_to_groups: vec![],
                shown: false,
            },
        ],
    }
}

fn sample_groups() -> Vec<String> {
    vec!["Family".into(), "Friends".into(), "Work".into()]
}

fn make_engine() -> ContactEditEngine {
    ContactEditEngine::new(sample_contact(), sample_groups())
}

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn edit_starts_at_fields() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "edit_fields");
    let progress = screen.progress.expect("should have progress");
    assert_eq!(progress.current_step, 1);
    assert_eq!(progress.total_steps, 3);
}

#[test]
fn edit_fields_has_name_input_and_field_list() {
    let engine = make_engine();
    let screen = engine.current_screen();

    // First component: TextInput for display_name
    match &screen.components[0] {
        Component::TextInput {
            id, label, value, ..
        } => {
            assert_eq!(id, "display_name");
            assert_eq!(label, "Display Name");
            assert_eq!(value, "Alice");
        }
        other => panic!("Expected TextInput, got {:?}", other),
    }

    // Second component: Divider
    assert_eq!(screen.components[1], Component::Divider);

    // Third component: FieldList
    match &screen.components[2] {
        Component::FieldList {
            id,
            fields,
            visibility_mode,
            available_groups,
            ..
        } => {
            assert_eq!(id, "fields");
            assert_eq!(fields.len(), 2);
            assert_eq!(visibility_mode, &VisibilityMode::ShowHide);
            assert_eq!(available_groups, &sample_groups());
        }
        other => panic!("Expected FieldList, got {:?}", other),
    }
}

#[test]
fn edit_change_name_updates_screen() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Bob".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => match &screen.components[0] {
            Component::TextInput { value, .. } => assert_eq!(value, "Bob"),
            other => panic!("Expected TextInput, got {:?}", other),
        },
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }

    assert_eq!(engine.edited_contact().display_name, "Bob");
}

#[test]
fn edit_empty_name_shows_validation_error() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "display_name");
            assert_eq!(message, "Name is required");
        }
        other => panic!("Expected ValidationError, got {:?}", other),
    }
}

#[test]
fn edit_continue_to_visibility() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "edit_visibility");
            let progress = screen.progress.expect("should have progress");
            assert_eq!(progress.current_step, 2);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

#[test]
fn edit_visibility_shows_toggle_lists() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "edit_visibility");

    // One ToggleList per field (2 fields)
    assert_eq!(screen.components.len(), 2);

    match &screen.components[0] {
        Component::ToggleList {
            id, label, items, ..
        } => {
            assert_eq!(id, "vis_phone1");
            assert_eq!(label, "Phone");
            assert_eq!(items.len(), 3); // Family, Friends, Work
            // Family should be selected (phone1 is visible to Family)
            assert!(
                items
                    .iter()
                    .find(|i| i.id == "Family")
                    .expect("Family")
                    .selected
            );
            assert!(
                !items
                    .iter()
                    .find(|i| i.id == "Friends")
                    .expect("Friends")
                    .selected
            );
        }
        other => panic!("Expected ToggleList, got {:?}", other),
    }

    match &screen.components[1] {
        Component::ToggleList {
            id, label, items, ..
        } => {
            assert_eq!(id, "vis_email1");
            assert_eq!(label, "Email");
            // email1 has no groups
            assert!(items.iter().all(|i| !i.selected));
        }
        other => panic!("Expected ToggleList, got {:?}", other),
    }
}

#[test]
fn edit_toggle_group_visibility() {
    let mut engine = make_engine();
    // Advance to visibility step
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Toggle "Friends" on for phone1
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "vis_phone1".into(),
        item_id: "Friends".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => match &screen.components[0] {
            Component::ToggleList { items, .. } => {
                let friends = items.iter().find(|i| i.id == "Friends").expect("Friends");
                assert!(friends.selected, "Friends should now be selected");
            }
            other => panic!("Expected ToggleList, got {:?}", other),
        },
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }

    // Verify data model updated
    let phone_field = engine
        .edited_contact()
        .fields
        .iter()
        .find(|f| f.id == "phone1")
        .expect("phone1");
    assert!(phone_field.visible_to_groups.contains(&"Friends".into()));
    assert!(phone_field.visible_to_groups.contains(&"Family".into()));
}

#[test]
fn edit_continue_to_preview() {
    let mut engine = make_engine();
    // Step 1 → 2
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // Step 2 → 3
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "edit_preview");
            let progress = screen.progress.expect("should have progress");
            assert_eq!(progress.current_step, 3);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

#[test]
fn edit_preview_shows_card() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "edit_preview");
    assert_eq!(screen.components.len(), 1);

    match &screen.components[0] {
        Component::CardPreview {
            name,
            fields,
            group_views,
            selected_group,
            ..
        } => {
            assert_eq!(name, "Alice");
            assert_eq!(fields.len(), 2);
            assert_eq!(group_views.len(), 3); // Family, Friends, Work
            assert!(selected_group.is_none());
        }
        other => panic!("Expected CardPreview, got {:?}", other),
    }
}

#[test]
fn edit_preview_group_selection() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::GroupViewSelected {
        group_name: Some("Family".into()),
    });

    match result {
        ActionResult::UpdateScreen(screen) => match &screen.components[0] {
            Component::CardPreview { selected_group, .. } => {
                assert_eq!(selected_group.as_deref(), Some("Family"));
            }
            other => panic!("Expected CardPreview, got {:?}", other),
        },
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

#[test]
fn edit_save_returns_complete() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn edit_back_from_visibility_to_fields() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "edit_fields");
            let progress = screen.progress.expect("should have progress");
            assert_eq!(progress.current_step, 1);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

#[test]
fn edit_back_from_preview_to_visibility() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "edit_visibility");
            let progress = screen.progress.expect("should have progress");
            assert_eq!(progress.current_step, 2);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

#[test]
fn edit_data_persists_across_steps() {
    let mut engine = make_engine();

    // Change name in step 1
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Updated Alice".into(),
    });

    // Step 1 → 2
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // Step 2 → 3
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Verify name is visible in preview
    let screen = engine.current_screen();
    match &screen.components[0] {
        Component::CardPreview { name, .. } => {
            assert_eq!(name, "Updated Alice");
        }
        other => panic!("Expected CardPreview, got {:?}", other),
    }

    // Also verify via accessor
    assert_eq!(engine.edited_contact().display_name, "Updated Alice");
}
