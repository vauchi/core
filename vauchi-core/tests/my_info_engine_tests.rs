// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn sample_own_fields() -> Vec<OwnFieldInfo> {
    vec![
        OwnFieldInfo {
            field_id: "f1".into(),
            field_type: "Phone".into(),
            label: "Mobile".into(),
            value: "+41 79 123 45 67".into(),
            visible_groups: vec!["Family".into()],
            contact_count: 3,
        },
        OwnFieldInfo {
            field_id: "f2".into(),
            field_type: "Email".into(),
            label: "Work".into(),
            value: "demo@vauchi.app".into(),
            visible_groups: vec![],
            contact_count: 0,
        },
    ]
}

#[test]
fn my_info_screen_id() {
    let engine = MyInfoEngine::new(MyInfoProgress::default());
    assert_eq!(engine.current_screen().screen_id, "my_info");
}

#[test]
fn my_info_shows_own_fields_in_action_list() {
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_own_card("Demo User".into(), sample_own_fields());

    let screen = engine.current_screen();
    let list = screen.components.iter().find(|c| {
        matches!(c, Component::ActionList { id, ..
        } if id == "own_entries")
    });
    assert!(list.is_some(), "MyInfo should show own entries ActionList");

    if let Some(Component::ActionList { items, .. }) = list {
        assert_eq!(items.len(), 2);
        assert!(items[0].label.contains("+41 79 123 45 67"));
        assert!(items[0].detail.as_deref().unwrap_or("").contains("Family"));
        assert!(
            items[0]
                .detail
                .as_deref()
                .unwrap_or("")
                .contains("3 contacts")
        );
    }
}

#[test]
fn my_info_title_is_display_name() {
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_own_card("Demo User".into(), sample_own_fields());

    let screen = engine.current_screen();
    assert_eq!(screen.title, "Demo User");
}

#[test]
fn my_info_has_add_entry_and_toggle_view_actions() {
    let engine = MyInfoEngine::new(MyInfoProgress::default());
    let screen = engine.current_screen();
    assert_eq!(screen.actions.len(), 3);
    assert_eq!(screen.actions[0].id, "add_field");
    assert_eq!(screen.actions[1].id, "toggle_view");
    assert_eq!(screen.actions[2].id, "preview-as-picker");
}

#[test]
fn my_info_no_setup_progress() {
    let engine = MyInfoEngine::new(MyInfoProgress {
        completed_steps: 3,
        total_steps: 6,
    });
    let screen = engine.current_screen();
    let has_progress = screen.components.iter().any(|c| {
        matches!(c, Component::StatusIndicator { id, ..
        } if id == "setup_progress")
    });
    assert!(!has_progress, "MyInfo should not show setup progress");
}

#[test]
fn my_info_empty_fields_shows_hint() {
    let engine = MyInfoEngine::new(MyInfoProgress::default());
    let screen = engine.current_screen();
    let has_hint = screen.components.iter().any(|c| {
        matches!(c, Component::Text { id, ..
        } if id == "empty_hint")
    });
    assert!(has_hint, "MyInfo without entries should show hint text");
}

#[test]
fn my_info_toggle_view_switches_mode() {
    let mut engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_own_card("Demo".into(), sample_own_fields());

    // Initially in entry view
    let screen = engine.current_screen();
    assert!(
        screen
            .actions
            .iter()
            .any(|a| a.id == "toggle_view" && a.label == "Group View")
    );

    // Toggle to group view
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "toggle_view".into(),
    });
    assert!(matches!(result, ActionResult::UpdateScreen(_)));

    let screen = engine.current_screen();
    assert!(
        screen
            .actions
            .iter()
            .any(|a| a.id == "toggle_view" && a.label == "Entry View")
    );
}

#[test]
fn my_info_entry_selection_opens_detail() {
    let mut engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_own_card("Demo".into(), sample_own_fields());

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "own_entries".into(),
        item_id: "f1".into(),
    });
    assert!(
        matches!(result, ActionResult::OpenEntryDetail { ref field_id } if field_id == "f1"),
        "Selecting entry should open entry detail, got {result:?}"
    );
}

fn sample_shared_info_view() -> SharedInfoView {
    SharedInfoView {
        shared_display_name: "Me (as Bob sees me)".into(),
        my_fields: vec![
            FieldDisplay {
                id: "mf1".into(),
                field_type: "Phone".into(),
                label: "Mobile".into(),
                value: "+41 79 123 45 67".into(),
                visibility: UiFieldVisibility::Shown,
            },
            FieldDisplay {
                id: "mf2".into(),
                field_type: "Email".into(),
                label: "Work".into(),
                value: "me@example.com".into(),
                visibility: UiFieldVisibility::Hidden,
            },
        ],
        visible_groups: vec!["Friends".into()],
    }
}

#[test]
fn test_preview_view_shows_banner_and_fields() {
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_own_card("Alice".into(), sample_own_fields())
        .with_preview(sample_shared_info_view())
        .with_view_mode(MyInfoViewMode::PreviewAs {
            contact_name: "Bob".into(),
        });

    let screen = engine.current_screen();

    // First component must be a Banner with "Viewing as Bob"
    let first = screen
        .components
        .first()
        .expect("Expected at least one component");
    match first {
        Component::Banner {
            text, action_id, ..
        } => {
            assert!(
                text.contains("Bob"),
                "Banner text should contain contact name, got: {text}"
            );
            assert_eq!(action_id, "exit-preview");
        }
        other => panic!("Expected Banner as first component, got: {other:?}"),
    }

    // An action with id "exit-preview" must be present
    let has_exit = screen.actions.iter().any(|a| a.id == "exit-preview");
    assert!(has_exit, "Expected exit-preview action in screen actions");

    // Visible field must have Shown marker, hidden field must have Hidden marker
    let shown_field = screen.components.iter().find(|c| {
        matches!(c, Component::FieldList { fields, ..
        }
            if fields.iter().any(|f| f.id == "mf1" && f.visibility == UiFieldVisibility::Shown))
    });
    assert!(
        shown_field.is_some(),
        "Expected visible field mf1 with Shown visibility"
    );

    let hidden_field = screen.components.iter().find(|c| {
        matches!(c, Component::FieldList { fields, ..
        }
            if fields.iter().any(|f| f.id == "mf2" && f.visibility == UiFieldVisibility::Hidden))
    });
    assert!(
        hidden_field.is_some(),
        "Expected hidden field mf2 with Hidden visibility"
    );
}
