// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

// --- AddField tests ---

/// Helper: select a category then a type in the multi-step AddField flow.
fn select_category_and_type(engine: &mut FormDialogEngine, category: &str, entry_type: &str) {
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "categories".into(),
        item_id: category.into(),
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: entry_type.into(),
    });
}

#[test]
fn form_dialog_add_field_shows_category_list() {
    let engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_add_field");

    let has_categories = screen.components.iter().any(|c| {
        matches!(c,
            Component::ActionList { id, .. } if id == "categories"
        )
    });
    assert!(has_categories, "Should show category list initially");
}

#[test]
fn form_dialog_add_field_category_shows_type_list() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "categories".into(),
        item_id: "Contact".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_types = screen.components.iter().any(|c| {
                matches!(c,
                    Component::ActionList { id, .. } if id == "entry_types"
                )
            });
            assert!(has_types, "Should show type list after selecting category");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_add_field_select_type_shows_value_input() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    select_category_and_type(&mut engine, "Contact", "email");

    let screen = engine.current_screen();
    let has_input = screen.components.iter().any(|c| {
        matches!(c,
            Component::TextInput { id, .. } if id == "field_value"
        )
    });
    assert!(has_input, "Should show value input after selecting type");
}

#[test]
fn form_dialog_add_field_select_type_shows_note_input() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    select_category_and_type(&mut engine, "Contact", "phone");

    let screen = engine.current_screen();
    let has_note = screen.components.iter().any(|c| {
        matches!(c,
            Component::TextInput { id, .. } if id == "field_note"
        )
    });
    assert!(has_note, "Should show note input after selecting type");
}

#[test]
fn form_dialog_add_field_submit_completes() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    select_category_and_type(&mut engine, "Contact", "email");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "test@example.com".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn form_dialog_add_field_cancel_with_type_deselects() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    select_category_and_type(&mut engine, "Contact", "email");

    // Cancel should deselect type (back to type list)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "form_add_field");
            let has_value_input = screen.components.iter().any(|c| {
                matches!(c,
                    Component::TextInput { id, .. } if id == "field_value"
                )
            });
            assert!(
                !has_value_input,
                "Should hide value input after cancel (type deselected)"
            );
            // Should show type list (still in category)
            let has_types = screen.components.iter().any(|c| {
                matches!(c,
                    Component::ActionList { id, .. } if id == "entry_types"
                )
            });
            assert!(
                has_types,
                "Should show type list after cancel from value step"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_add_field_cancel_from_type_list_goes_to_categories() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    // Select category
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "categories".into(),
        item_id: "Contact".into(),
    });

    // Cancel from type list → back to categories
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_categories = screen.components.iter().any(|c| {
                matches!(c,
                    Component::ActionList { id, .. } if id == "categories"
                )
            });
            assert!(
                has_categories,
                "Should show category list after cancel from type list"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_add_field_cancel_from_categories_navigates() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    // Cancel from category list → NavigateTo (exit)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "Cancel from categories should navigate away, got {result:?}"
    );
}

#[test]
fn form_dialog_add_field_collected_input_format() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    select_category_and_type(&mut engine, "Contact", "email");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "test@example.com".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_note".into(),
        value: "work".into(),
    });

    let input = engine
        .collected_input()
        .expect("collected_input should return Some");
    let parts: Vec<&str> = input.split('\n').collect();
    assert_eq!(
        parts.len(),
        4,
        "Format should be type\\nnote\\nvalue\\ngroups"
    );
    assert_eq!(parts[0], "email");
    assert_eq!(parts[1], "work");
    assert_eq!(parts[2], "test@example.com");
    assert_eq!(parts[3], "", "No groups selected");
}

#[test]
fn form_dialog_add_field_group_toggle() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![
            ("g1".into(), "Family".into()),
            ("g2".into(), "Friends".into()),
        ],
    });

    select_category_and_type(&mut engine, "Contact", "phone");

    // Toggle a group
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_visibility".into(),
        item_id: "g1".into(),
    });
    match &result {
        ActionResult::UpdateScreen(screen) => {
            let has_toggle_list = screen
                .components
                .iter()
                .any(|c| matches!(c, Component::ToggleList { id, .. } if id == "group_visibility"));
            assert!(has_toggle_list, "Should show group visibility toggles");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }

    // Verify collected_input includes groups
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+1 555".into(),
    });
    let input = engine
        .collected_input()
        .expect("collected_input should return Some");
    let parts: Vec<&str> = input.split('\n').collect();
    assert_eq!(parts[3], "g1", "Should include toggled group");
}

#[test]
fn form_dialog_add_field_custom_category_skips_type_selection() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    // Select Custom category — should skip type list (single entry)
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "categories".into(),
        item_id: "Custom".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            // Should go straight to value input (skip type selection)
            let has_value = screen.components.iter().any(|c| {
                matches!(c,
                    Component::TextInput { id, .. } if id == "field_value"
                )
            });
            assert!(
                has_value,
                "Custom category should skip type list and show value input"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// --- EditField tests ---

#[test]
fn form_dialog_edit_field_screen_id() {
    let engine = FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Email".into(),
        current_value: "old@example.com".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_edit_field");
}

#[test]
fn form_dialog_edit_field_prefills_current_value() {
    let engine = FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Email".into(),
        current_value: "old@example.com".into(),
    });
    let screen = engine.current_screen();

    let prefilled = screen.components.iter().any(|c| {
        matches!(c,
            Component::TextInput { id, value, .. } if id == "field_value" && value == "old@example.com"
        )
    });
    assert!(
        prefilled,
        "field_value input should be prefilled with 'old@example.com'"
    );
}

#[test]
fn form_dialog_edit_field_submit_completes() {
    let mut engine = FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Email".into(),
        current_value: String::new(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "new@example.com".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn form_dialog_edit_field_collected_input() {
    let mut engine = FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Email".into(),
        current_value: String::new(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "updated@example.com".into(),
    });
    let input = engine
        .collected_input()
        .expect("collected_input should return Some");
    assert_eq!(input, "updated@example.com");
}

// --- EditName tests ---

#[test]
fn form_dialog_edit_name_screen_id() {
    let engine = FormDialogEngine::new(FormDialogType::EditName {
        current_name: "Alice".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_edit_name");
}

#[test]
fn form_dialog_edit_name_prefills_current_name() {
    let engine = FormDialogEngine::new(FormDialogType::EditName {
        current_name: "Alice".into(),
    });
    let screen = engine.current_screen();

    let prefilled = screen.components.iter().any(|c| {
        matches!(c,
            Component::TextInput { id, value, .. } if id == "display_name" && value == "Alice"
        )
    });
    assert!(
        prefilled,
        "display_name input should be prefilled with 'Alice'"
    );
}

#[test]
fn form_dialog_edit_name_collected_input() {
    let mut engine = FormDialogEngine::new(FormDialogType::EditName {
        current_name: "Alice".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Bob".into(),
    });
    let input = engine
        .collected_input()
        .expect("collected_input should return Some");
    assert_eq!(input, "Bob");
}

// --- EditRelayUrl tests ---

#[test]
fn form_dialog_edit_relay_url_screen_id() {
    let engine = FormDialogEngine::new(FormDialogType::EditRelayUrl {
        current_url: "wss://relay.vauchi.app".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_edit_relay_url");
}

#[test]
fn form_dialog_edit_relay_url_collected_input() {
    let mut engine = FormDialogEngine::new(FormDialogType::EditRelayUrl {
        current_url: "wss://relay.vauchi.app".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "relay_url".into(),
        value: "wss://custom.relay.example".into(),
    });
    let input = engine
        .collected_input()
        .expect("collected_input should return Some");
    assert_eq!(input, "wss://custom.relay.example");
}

// --- TextChanged test ---

#[test]
fn form_dialog_text_changed_updates_value() {
    let mut engine = FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Phone".into(),
        current_value: String::new(),
    });

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+1 555 000 1234".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_updated_value = screen.components.iter().any(|c| matches!(c,
                Component::TextInput { id, value, .. } if id == "field_value" && value == "+1 555 000 1234"
            ));
            assert!(
                has_updated_value,
                "TextInput should reflect the updated value"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}
