// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::{A11y, AccessibilityRole, SettingsItem, *};

// @internal
#[test]
fn test_screen_model_serde_roundtrip() {
    let screen = ScreenModel {
        screen_id: "default_name".to_string(),
        title: "What's your name?".to_string(),
        subtitle: Some("This is how contacts will see you.".to_string()),
        components: vec![
            Component::InfoPanel {
                id: "info".to_string(),
                icon: None,
                title: "What is Vauchi?".to_string(),
                items: vec![InfoItem {
                    icon: None,
                    title: "Privacy".to_string(),
                    detail: "Your data stays yours".to_string(),
                }],
                a11y: None,
            },
            Component::Divider,
        ],
        actions: vec![ScreenAction {
            id: "continue".to_string(),
            label: "Continue".to_string(),
            style: ActionStyle::Primary,
            enabled: true,
        }],
        progress: Some(Progress {
            current_step: 1,
            total_steps: 4,
            label: Some("DefaultName".to_string()),
        }),
        ..Default::default()
    };

    let json = serde_json::to_string(&screen).unwrap();
    let restored: ScreenModel = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.screen_id, "default_name");
    assert_eq!(restored.title, "What's your name?");
    assert_eq!(restored.components.len(), 2);
    assert_eq!(restored.actions.len(), 1);
    assert!(restored.progress.is_some(), "expected Some value");
    let progress = restored.progress.unwrap();
    assert_eq!(progress.current_step, 1);
    assert_eq!(progress.total_steps, 4);
    assert_eq!(progress.label.as_deref(), Some("DefaultName"));
}

// @internal
#[test]
fn test_user_action_text_changed_roundtrip() {
    let action = UserAction::TextChanged {
        component_id: "name".into(),
        value: "Alice".into(),
    };
    let json = serde_json::to_string(&action).unwrap();
    let restored: UserAction = serde_json::from_str(&json).unwrap();
    match restored {
        UserAction::TextChanged {
            component_id,
            value,
        } => {
            assert_eq!(component_id, "name");
            assert_eq!(value, "Alice");
        }
        other => panic!("Expected TextChanged, got {:?}", other),
    }
}

// @internal
#[test]
fn test_user_action_item_toggled_roundtrip() {
    let action = UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "family".into(),
    };
    let json = serde_json::to_string(&action).unwrap();
    let restored: UserAction = serde_json::from_str(&json).unwrap();
    match restored {
        UserAction::ItemToggled {
            component_id,
            item_id,
        } => {
            assert_eq!(component_id, "groups");
            assert_eq!(item_id, "family");
        }
        other => panic!("Expected ItemToggled, got {:?}", other),
    }
}

// @internal
#[test]
fn test_user_action_action_pressed_roundtrip() {
    let action = UserAction::ActionPressed {
        action_id: "continue".into(),
    };
    let json = serde_json::to_string(&action).unwrap();
    let restored: UserAction = serde_json::from_str(&json).unwrap();
    match restored {
        UserAction::ActionPressed { action_id } => {
            assert_eq!(action_id, "continue");
        }
        other => panic!("Expected ActionPressed, got {:?}", other),
    }
}

// @internal
#[test]
fn test_user_action_field_visibility_changed_roundtrip() {
    let action = UserAction::FieldVisibilityChanged {
        field_id: "email".into(),
        group_id: Some("friends".into()),
        visible: true,
    };
    let json = serde_json::to_string(&action).unwrap();
    let restored: UserAction = serde_json::from_str(&json).unwrap();
    match restored {
        UserAction::FieldVisibilityChanged {
            field_id,
            group_id,
            visible,
        } => {
            assert_eq!(field_id, "email");
            assert_eq!(group_id.as_deref(), Some("friends"));
            assert!(visible);
        }
        other => panic!("Expected FieldVisibilityChanged, got {:?}", other),
    }
}

// @internal
#[test]
fn test_user_action_group_view_selected_roundtrip() {
    let action = UserAction::GroupViewSelected {
        group_name: Some("Family".into()),
    };
    let json = serde_json::to_string(&action).unwrap();
    let restored: UserAction = serde_json::from_str(&json).unwrap();
    match restored {
        UserAction::GroupViewSelected { group_name } => {
            assert_eq!(group_name.as_deref(), Some("Family"));
        }
        other => panic!("Expected GroupViewSelected, got {:?}", other),
    }
}

// @internal
#[test]
fn test_action_result_update_screen_roundtrip() {
    let screen = ScreenModel {
        screen_id: "test".into(),
        title: "Test".into(),
        subtitle: None,
        components: vec![],
        actions: vec![],
        progress: None,
        ..Default::default()
    };

    let result = ActionResult::UpdateScreen(screen);
    let json = serde_json::to_string(&result).unwrap();
    let restored: ActionResult = serde_json::from_str(&json).unwrap();
    match restored {
        ActionResult::UpdateScreen(s) => {
            assert_eq!(s.screen_id, "test");
            assert_eq!(s.title, "Test");
            assert!(s.subtitle.is_none());
            assert!(s.components.is_empty());
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// @internal
#[test]
fn test_action_result_navigate_to_roundtrip() {
    let screen = ScreenModel {
        screen_id: "next".into(),
        title: "Next".into(),
        subtitle: None,
        components: vec![],
        actions: vec![],
        progress: None,
        ..Default::default()
    };

    let result = ActionResult::NavigateTo(screen);
    let json = serde_json::to_string(&result).unwrap();
    let restored: ActionResult = serde_json::from_str(&json).unwrap();
    match restored {
        ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "next"),
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn test_action_result_validation_error_roundtrip() {
    let result = ActionResult::ValidationError {
        component_id: "name".into(),
        message: "Required".into(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let restored: ActionResult = serde_json::from_str(&json).unwrap();
    match restored {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "name");
            assert_eq!(message, "Required");
        }
        other => panic!("Expected ValidationError, got {:?}", other),
    }
}

// @internal
#[test]
fn test_action_result_complete_roundtrip() {
    let result = ActionResult::Complete;
    let json = serde_json::to_string(&result).unwrap();
    let restored: ActionResult = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, ActionResult::Complete));
}

// @internal
#[test]
fn test_component_text_input_roundtrip() {
    let component = Component::TextInput {
        id: "display_name".into(),
        label: "Display Name".into(),
        value: "Alice".into(),
        placeholder: Some("Enter your name".into()),
        max_length: Some(50),
        validation_error: None,
        input_type: InputType::Text,
        a11y: None,
    };

    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    match restored {
        Component::TextInput {
            id,
            label,
            value,
            placeholder,
            max_length,
            validation_error,
            input_type,
            ..
        } => {
            assert_eq!(id, "display_name");
            assert_eq!(label, "Display Name");
            assert_eq!(value, "Alice");
            assert_eq!(placeholder.as_deref(), Some("Enter your name"));
            assert_eq!(max_length, Some(50));
            assert!(validation_error.is_none());
            assert_eq!(input_type, InputType::Text);
        }
        other => panic!("Expected TextInput, got {:?}", other),
    }
}

// @internal
#[test]
fn test_component_toggle_list_roundtrip() {
    let component = Component::ToggleList {
        id: "groups".into(),
        label: "Select Groups".into(),
        items: vec![
            ToggleItem {
                id: "family".into(),
                label: "Family".into(),
                selected: true,
                subtitle: Some("Close family members".into()),
                a11y: None,
            },
            ToggleItem {
                id: "work".into(),
                label: "Work".into(),
                selected: false,
                subtitle: None,
                a11y: None,
            },
        ],
        a11y: None,
    };

    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    match restored {
        Component::ToggleList {
            id, label, items, ..
        } => {
            assert_eq!(id, "groups");
            assert_eq!(label, "Select Groups");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].id, "family");
            assert!(items[0].selected);
            assert_eq!(items[0].subtitle.as_deref(), Some("Close family members"));
            assert_eq!(items[1].id, "work");
            assert!(!items[1].selected);
            assert!(items[1].subtitle.is_none());
        }
        other => panic!("Expected ToggleList, got {:?}", other),
    }
}

// @internal
#[test]
fn test_component_field_list_roundtrip() {
    let component = Component::FieldList {
        id: "fields".into(),
        fields: vec![FieldDisplay {
            id: "email".into(),
            field_type: "email".into(),
            label: "Email".into(),
            value: "alice@example.com".into(),
            visibility: UiFieldVisibility::Groups(vec!["friends".into()]),
            a11y: None,
        }],
        visibility_mode: VisibilityMode::PerGroup,
        available_groups: vec!["friends".into(), "family".into()],
        a11y: None,
    };

    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    match restored {
        Component::FieldList {
            id,
            fields,
            visibility_mode,
            available_groups,
            ..
        } => {
            assert_eq!(id, "fields");
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].id, "email");
            assert_eq!(fields[0].value, "alice@example.com");
            assert_eq!(
                fields[0].visibility,
                UiFieldVisibility::Groups(vec!["friends".into()])
            );
            assert_eq!(visibility_mode, VisibilityMode::PerGroup);
            assert_eq!(available_groups, vec!["friends", "family"]);
        }
        other => panic!("Expected FieldList, got {:?}", other),
    }
}

// @internal
#[test]
fn test_component_card_preview_roundtrip() {
    let component = Component::CardPreview {
        name: "Alice".into(),
        avatar_data: None,
        fields: vec![FieldDisplay {
            id: "phone".into(),
            field_type: "phone".into(),
            label: "Phone".into(),
            value: "+1234567890".into(),
            visibility: UiFieldVisibility::Shown,
            a11y: None,
        }],
        group_views: vec![GroupCardView {
            group_name: "family".into(),
            display_name: "Family".into(),
            visible_fields: vec![FieldDisplay {
                id: "phone".into(),
                field_type: "phone".into(),
                label: "Phone".into(),
                value: "+1234567890".into(),
                visibility: UiFieldVisibility::Shown,
                a11y: None,
            }],
        }],
        selected_group: Some("family".into()),
        a11y: None,
    };

    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    match restored {
        Component::CardPreview {
            name,
            fields,
            group_views,
            selected_group,
            ..
        } => {
            assert_eq!(name, "Alice");
            assert_eq!(fields.len(), 1);
            assert_eq!(group_views.len(), 1);
            assert_eq!(group_views[0].group_name, "family");
            assert_eq!(group_views[0].display_name, "Family");
            assert_eq!(selected_group.as_deref(), Some("family"));
        }
        other => panic!("Expected CardPreview, got {:?}", other),
    }
}

// @internal
#[test]
fn test_screen_action_styles() {
    let styles = vec![
        ActionStyle::Primary,
        ActionStyle::Secondary,
        ActionStyle::Destructive,
    ];

    for style in styles {
        let action = ScreenAction {
            id: "test".into(),
            label: "Test".into(),
            style,
            enabled: true,
        };
        let json = serde_json::to_string(&action).unwrap();
        let restored: ScreenAction = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, "test");
        assert!(restored.enabled);
    }
}

// @internal
#[test]
fn test_text_style_variants() {
    let styles = vec![
        TextStyle::Title,
        TextStyle::Subtitle,
        TextStyle::Body,
        TextStyle::Caption,
    ];

    for style in &styles {
        let component = Component::Text {
            id: "t".into(),
            content: "hello".into(),
            style: style.clone(),
        };
        let json = serde_json::to_string(&component).unwrap();
        let restored: Component = serde_json::from_str(&json).unwrap();
        match restored {
            Component::Text {
                style: restored_style,
                ..
            } => assert_eq!(restored_style, *style),
            other => panic!("Expected Text, got {:?}", other),
        }
    }
}

// @internal
#[test]
fn test_input_type_variants() {
    let types = vec![InputType::Text, InputType::Phone, InputType::Email];

    for input_type in &types {
        let json = serde_json::to_string(input_type).unwrap();
        let restored: InputType = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, *input_type);
    }
}

// @internal
#[test]
fn test_field_visibility_variants() {
    let variants = vec![
        UiFieldVisibility::Shown,
        UiFieldVisibility::Hidden,
        UiFieldVisibility::Groups(vec!["a".into(), "b".into()]),
    ];

    for vis in &variants {
        let json = serde_json::to_string(vis).unwrap();
        let restored: UiFieldVisibility = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, *vis);
    }
}

// @internal
#[test]
fn test_divider_component_roundtrip() {
    let component = Component::Divider;
    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Component::Divider));
}

// @internal
#[test]
fn test_screen_model_empty_components() {
    let screen = ScreenModel {
        screen_id: "empty".into(),
        title: "Empty".into(),
        subtitle: None,
        components: vec![],
        actions: vec![],
        progress: None,
        ..Default::default()
    };

    let json = serde_json::to_string(&screen).unwrap();
    let restored: ScreenModel = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.screen_id, "empty");
    assert!(restored.components.is_empty());
    assert!(restored.actions.is_empty());
    assert!(restored.progress.is_none());
    assert!(restored.subtitle.is_none());
}

// @internal
#[test]
fn test_screen_action_disabled() {
    let action = ScreenAction {
        id: "submit".into(),
        label: "Submit".into(),
        style: ActionStyle::Primary,
        enabled: false,
    };

    let json = serde_json::to_string(&action).unwrap();
    let restored: ScreenAction = serde_json::from_str(&json).unwrap();
    assert!(!restored.enabled);
}

// === InlineConfirm component ===

// @internal
#[test]
fn test_component_inline_confirm_roundtrip() {
    let component = Component::InlineConfirm {
        id: "confirm-1".into(),
        warning: "This permanently deletes your identity".into(),
        confirm_text: "Delete Forever".into(),
        cancel_text: "Cancel".into(),
        destructive: true,
        a11y: None,
    };

    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    match restored {
        Component::InlineConfirm {
            id,
            warning,
            confirm_text,
            cancel_text,
            destructive,
            ..
        } => {
            assert_eq!(id, "confirm-1");
            assert_eq!(warning, "This permanently deletes your identity");
            assert_eq!(confirm_text, "Delete Forever");
            assert_eq!(cancel_text, "Cancel");
            assert!(destructive);
        }
        other => panic!("Expected InlineConfirm, got {:?}", other),
    }
}

// @internal
#[test]
fn test_component_inline_confirm_non_destructive_roundtrip() {
    let component = Component::InlineConfirm {
        id: "confirm-2".into(),
        warning: "Are you sure?".into(),
        confirm_text: "Yes".into(),
        cancel_text: "No".into(),
        destructive: false,
        a11y: None,
    };

    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    match restored {
        Component::InlineConfirm {
            id,
            warning,
            confirm_text,
            cancel_text,
            destructive,
            ..
        } => {
            assert_eq!(id, "confirm-2");
            assert_eq!(warning, "Are you sure?");
            assert_eq!(confirm_text, "Yes");
            assert_eq!(cancel_text, "No");
            assert!(!destructive);
        }
        other => panic!("Expected InlineConfirm, got {:?}", other),
    }
}

// === EditableText component ===

// @internal
#[test]
fn test_component_editable_text_display_mode_roundtrip() {
    let component = Component::EditableText {
        id: "edit-name".into(),
        label: "Display Name".into(),
        value: "Alice".into(),
        editing: false,
        validation_error: None,
        a11y: None,
    };

    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    match restored {
        Component::EditableText {
            id,
            label,
            value,
            editing,
            validation_error,
            ..
        } => {
            assert_eq!(id, "edit-name");
            assert_eq!(label, "Display Name");
            assert_eq!(value, "Alice");
            assert!(!editing);
            assert!(validation_error.is_none());
        }
        other => panic!("Expected EditableText, got {:?}", other),
    }
}

// @internal
#[test]
fn test_component_editable_text_editing_with_error_roundtrip() {
    let component = Component::EditableText {
        id: "edit-name".into(),
        label: "Display Name".into(),
        value: "".into(),
        editing: true,
        validation_error: Some("Name cannot be empty".into()),
        a11y: None,
    };

    let json = serde_json::to_string(&component).unwrap();
    let restored: Component = serde_json::from_str(&json).unwrap();
    match restored {
        Component::EditableText {
            id,
            label,
            value,
            editing,
            validation_error,
            ..
        } => {
            assert_eq!(id, "edit-name");
            assert_eq!(label, "Display Name");
            assert_eq!(value, "");
            assert!(editing);
            assert_eq!(validation_error.as_deref(), Some("Name cannot be empty"));
        }
        other => panic!("Expected EditableText, got {:?}", other),
    }
}

// === ShowToast ActionResult ===

// @internal
#[test]
fn test_action_result_show_toast_roundtrip() {
    let result = ActionResult::ShowToast {
        message: "Contact deleted".into(),
        undo_action_id: Some("undo-delete-alice".into()),
    };
    let json = serde_json::to_string(&result).unwrap();
    let restored: ActionResult = serde_json::from_str(&json).unwrap();
    match restored {
        ActionResult::ShowToast {
            message,
            undo_action_id,
        } => {
            assert_eq!(message, "Contact deleted");
            assert_eq!(undo_action_id.as_deref(), Some("undo-delete-alice"));
        }
        other => panic!("Expected ShowToast, got {:?}", other),
    }
}

// @internal
#[test]
fn test_action_result_show_toast_without_undo_roundtrip() {
    let result = ActionResult::ShowToast {
        message: "Changes saved".into(),
        undo_action_id: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    let restored: ActionResult = serde_json::from_str(&json).unwrap();
    match restored {
        ActionResult::ShowToast {
            message,
            undo_action_id,
        } => {
            assert_eq!(message, "Changes saved");
            assert!(undo_action_id.is_none());
        }
        other => panic!("Expected ShowToast, got {:?}", other),
    }
}

// === UndoPressed UserAction ===

// @internal
#[test]
fn test_user_action_undo_pressed_roundtrip() {
    let action = UserAction::UndoPressed {
        action_id: "undo-delete-alice".into(),
    };
    let json = serde_json::to_string(&action).unwrap();
    let restored: UserAction = serde_json::from_str(&json).unwrap();
    match restored {
        UserAction::UndoPressed { action_id } => {
            assert_eq!(action_id, "undo-delete-alice");
        }
        other => panic!("Expected UndoPressed, got {:?}", other),
    }
}

// === A11y struct ===

// @internal
// @internal
#[test]
fn test_a11y_struct_roundtrip() {
    let a11y = A11y {
        label: Some("Submit button".into()),
        hint: Some("Double tap to submit the form".into()),
        role: None,
    };
    let json = serde_json::to_string(&a11y).unwrap();
    let parsed: A11y = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.label.as_deref(), Some("Submit button"));
    assert_eq!(
        parsed.hint.as_deref(),
        Some("Double tap to submit the form")
    );
    assert_eq!(parsed.role, None);
}

// @internal
// @internal
#[test]
fn test_a11y_default_is_none() {
    let a11y = A11y::default();
    assert_eq!(a11y.label, None);
    assert_eq!(a11y.hint, None);
    assert_eq!(a11y.role, None);
}

// @internal
// @internal
#[test]
fn test_a11y_with_role_roundtrip() {
    let a11y = A11y {
        label: Some("Submit".into()),
        hint: None,
        role: Some(AccessibilityRole::Button),
    };
    let json = serde_json::to_string(&a11y).unwrap();
    let parsed: A11y = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, Some(AccessibilityRole::Button));
}

// @internal
// @internal
#[test]
fn test_a11y_without_role_deserializes() {
    let json = r#"{"label":"test"}"#;
    let a11y: A11y = serde_json::from_str(json).unwrap();
    assert_eq!(a11y.role, None);
}

// === Component a11y field backward-compat ===

// === Sub-type struct a11y field backward-compat ===

// @internal
// @internal
#[test]
fn test_settings_item_without_a11y_deserializes() {
    let json = r#"{"id":"theme","label":"Theme","kind":{"Link":{"detail":"Catppuccin"}}}"#;
    let item: SettingsItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.id, "theme");
    assert_eq!(item.a11y, None);
}

// @internal
// @internal
#[test]
fn test_component_text_input_without_a11y_deserializes() {
    let json = r#"{"TextInput":{"id":"name","label":"Name","value":"","placeholder":"Enter name","max_length":null,"validation_error":null,"input_type":"Text"}}"#;
    let component: Component = serde_json::from_str(json).unwrap();
    match component {
        Component::TextInput { a11y, .. } => assert_eq!(a11y, None),
        other => panic!("Expected TextInput, got {:?}", other),
    }
}

// @internal
// @internal
#[test]
fn test_component_text_input_with_a11y_roundtrip() {
    let component = Component::TextInput {
        id: "name".into(),
        label: "Name".into(),
        value: "Alice".into(),
        placeholder: Some("Enter name".into()),
        max_length: None,
        validation_error: None,
        input_type: InputType::Text,
        a11y: Some(A11y {
            label: Some("Name field".into()),
            hint: Some("Enter your display name".into()),
            role: None,
        }),
    };
    let json = serde_json::to_string(&component).unwrap();
    let parsed: Component = serde_json::from_str(&json).unwrap();
    assert_eq!(component, parsed);
    assert!(json.contains("\"a11y\""));
}
