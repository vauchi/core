// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

// --- AddField tests ---

#[test]
fn form_dialog_add_field_shows_category_picker() {
    let engine = FormDialogEngine::new(FormDialogType::AddField);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_add_field_type");

    let has_categories = screen.components.iter().any(|c| {
        matches!(c,
            Component::ActionList { id, .. } if id == "entry_categories"
        )
    });
    assert!(has_categories, "Should show category picker initially");
}

#[test]
fn form_dialog_add_field_select_category_shows_types() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField);

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_categories".into(),
        item_id: "contact".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "form_add_field_type");
            let has_types = screen.components.iter().any(|c| {
                matches!(c,
                    Component::ActionList { id, .. } if id == "entry_types"
                )
            });
            assert!(
                has_types,
                "Should show entry types after selecting category"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_add_field_select_type_shows_value_input() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField);

    // Select category
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_categories".into(),
        item_id: "contact".into(),
    });

    // Select type
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "form_add_field");
            let has_input = screen.components.iter().any(|c| {
                matches!(c,
                    Component::TextInput { id, .. } if id == "field_value"
                )
            });
            assert!(has_input, "Should show value input after selecting type");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_add_field_submit_completes() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField);

    // Select category, type, enter value
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_categories".into(),
        item_id: "contact".into(),
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
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
fn form_dialog_add_field_cancel_from_value_goes_to_type_picker() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField);

    // Navigate to value input
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_categories".into(),
        item_id: "contact".into(),
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });

    // Cancel should go back to type picker
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "form_add_field_type");
            let has_types = screen.components.iter().any(|c| {
                matches!(c,
                    Component::ActionList { id, .. } if id == "entry_types"
                )
            });
            assert!(
                has_types,
                "Should go back to type picker on cancel from value"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_add_field_back_to_categories() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField);

    // Select category first
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_categories".into(),
        item_id: "contact".into(),
    });

    // Use back_to_categories action
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back_to_categories".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_categories = screen.components.iter().any(|c| {
                matches!(c,
                    Component::ActionList { id, .. } if id == "entry_categories"
                )
            });
            assert!(
                has_categories,
                "Should show categories again after back_to_categories"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_add_field_collected_input_format() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField);

    // Select category and type
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_categories".into(),
        item_id: "contact".into(),
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
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
    assert_eq!(parts.len(), 3, "Format should be type\\nnote\\nvalue");
    assert_eq!(parts[0], "email");
    assert_eq!(parts[1], "work");
    assert_eq!(parts[2], "test@example.com");
}

// --- EditField tests ---

#[test]
fn form_dialog_edit_field_screen_id() {
    let engine = FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Email".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_edit_field");
}

#[test]
fn form_dialog_edit_field_submit_completes() {
    let mut engine = FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Email".into(),
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
