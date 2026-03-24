// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// ── Test helpers ────────────────────────────────────────────────────

fn sample_fields() -> Vec<ToggleItem> {
    vec![
        ToggleItem {
            id: "email".into(),
            label: "Email".into(),
            selected: true,
            subtitle: None,
        },
        ToggleItem {
            id: "phone".into(),
            label: "Phone".into(),
            selected: false,
            subtitle: None,
        },
    ]
}

fn make_engine() -> ContactVisibilityEngine {
    ContactVisibilityEngine::new("c1".into(), "Alice".into(), sample_fields())
}

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn contact_visibility_screen_id() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_visibility");
}

#[test]
fn contact_visibility_title_includes_contact_name() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert!(
        screen.title.contains("Alice"),
        "Expected title to contain 'Alice', got: {}",
        screen.title
    );
}

#[test]
fn contact_visibility_toggle_flips_field_state() {
    let mut engine = make_engine();
    // Phone starts as not selected (hidden)
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "field_toggles".into(),
        item_id: "phone".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => match &screen.components[1] {
            Component::ToggleList { items, .. } => {
                let phone = items.iter().find(|i| i.id == "phone").expect("phone item");
                assert!(phone.selected, "Phone should now be selected (visible)");
            }
            other => panic!("Expected ToggleList, got {:?}", other),
        },
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

#[test]
fn contact_visibility_save_completes() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn contact_visibility_collected_input_format() {
    let engine = make_engine();
    let input = engine
        .collected_input()
        .expect("should return collected input");
    assert_eq!(input, "email:visible,phone:hidden");
}

#[test]
fn contact_visibility_toggle_then_collected_input_reflects_change() {
    let mut engine = make_engine();
    // Toggle phone from hidden to visible
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "field_toggles".into(),
        item_id: "phone".into(),
    });

    let input = engine
        .collected_input()
        .expect("should return collected input");
    assert_eq!(input, "email:visible,phone:visible");
}

#[test]
fn contact_visibility_unknown_action_returns_update_screen() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_visibility");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}
