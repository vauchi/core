// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// --- AddField tests ---

#[test]
fn form_dialog_add_field_shows_type_list_and_inputs() {
    let engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_add_field");
    assert_eq!(screen.title, "Add Entry to MyInfo");

    // All components visible on single page
    let has_types = screen.components.iter().any(|c| {
        matches!(c,
            Component::ActionList { id, .. } if id == "entry_types"
        )
    });
    assert!(has_types, "Should show entry types list");

    let has_value = screen.components.iter().any(|c| {
        matches!(c,
            Component::TextInput { id, .. } if id == "field_value"
        )
    });
    assert!(has_value, "Should show value input");

    let has_label = screen.components.iter().any(|c| {
        matches!(c,
            Component::TextInput { id, label, .. } if id == "field_label" && label.contains("Display Name")
        )
    });
    assert!(has_label, "Should show Display Name input");

    let has_note = screen.components.iter().any(|c| {
        matches!(c,
            Component::TextInput { id, label, .. } if id == "field_note" && label.contains("Comment")
        )
    });
    assert!(has_note, "Should show Comment input");
}

#[test]
fn form_dialog_add_field_select_type_updates_title() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert!(
                screen.title.contains("Email"),
                "Title should mention selected type, got: {}",
                screen.title
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_add_field_submit_completes() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
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
fn form_dialog_add_field_cancel_navigates() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert_eq!(
        result,
        ActionResult::Complete,
        "Cancel should return Complete"
    );
    assert!(
        engine.was_cancelled(),
        "Engine should be marked as cancelled"
    );
}

#[test]
fn form_dialog_add_field_collected_input_format() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
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
        component_id: "field_label".into(),
        value: "Work".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_note".into(),
        value: "main account".into(),
    });

    let input = engine
        .collected_input()
        .expect("collected_input should return Some");
    let parts: Vec<&str> = input.split('\n').collect();
    assert_eq!(
        parts.len(),
        5,
        "Format should be type\\nlabel\\nvalue\\nnote\\ngroups"
    );
    assert_eq!(parts[0], "email");
    assert_eq!(parts[1], "Work");
    assert_eq!(parts[2], "test@example.com");
    assert_eq!(parts[3], "main account");
    assert_eq!(parts[4], "", "No groups selected");
}

#[test]
fn form_dialog_add_field_group_toggle() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![
            ("g1".into(), "Family".into()),
            ("g2".into(), "Friends".into()),
        ],
    });

    // Groups should be visible immediately (single-page form)
    let screen = engine.current_screen();
    let has_toggle_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ToggleList { id, .. } if id == "group_visibility"));
    assert!(has_toggle_list, "Should show group visibility toggles");

    // Toggle a group
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_visibility".into(),
        item_id: "g1".into(),
    });

    // Select type and add value for collected_input
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "phone".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+1 555".into(),
    });
    let input = engine
        .collected_input()
        .expect("collected_input should return Some");
    let parts: Vec<&str> = input.split('\n').collect();
    assert_eq!(parts[4], "g1", "Should include toggled group");
}

// --- EditField tests ---

#[test]
fn form_dialog_edit_field_screen_id() {
    let engine = FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Email".into(),
        current_value: "old@example.com".into(),
        current_note: None,
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
        current_note: None,
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
        current_note: None,
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
        current_note: None,
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "updated@example.com".into(),
    });
    let input = engine
        .collected_input()
        .expect("collected_input should return Some");
    // Format: "value\nnote" — note is empty so input starts with value
    let value_part = input.splitn(2, '\n').next().unwrap_or("");
    assert_eq!(value_part, "updated@example.com");
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
        current_note: None,
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
