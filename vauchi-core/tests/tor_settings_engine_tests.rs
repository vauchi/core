// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

#[test]
fn tor_settings_screen_id() {
    let engine = TorSettingsEngine::new(false, false);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "tor_settings");
}

#[test]
fn tor_settings_title() {
    let engine = TorSettingsEngine::new(false, false);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Tor Privacy");
}

#[test]
fn tor_settings_toggle_tor_enabled() {
    let mut engine = TorSettingsEngine::new(false, false);

    // Verify initially disabled
    let screen = engine.current_screen();
    let tor_toggle = find_toggle(&screen, "tor_toggles", "tor_enabled");
    assert!(!tor_toggle, "tor_enabled should start disabled");

    // Toggle on
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "tor_toggles".into(),
        item_id: "tor_enabled".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let toggled = find_toggle(&screen, "tor_toggles", "tor_enabled");
            assert!(toggled, "tor_enabled should be enabled after toggle");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn tor_settings_toggle_prefer_onion() {
    let mut engine = TorSettingsEngine::new(true, false);

    // Verify initially disabled
    let screen = engine.current_screen();
    let onion_toggle = find_toggle(&screen, "tor_toggles", "prefer_onion");
    assert!(!onion_toggle, "prefer_onion should start disabled");

    // Toggle on
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "tor_toggles".into(),
        item_id: "prefer_onion".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let toggled = find_toggle(&screen, "tor_toggles", "prefer_onion");
            assert!(toggled, "prefer_onion should be enabled after toggle");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn tor_settings_new_circuit_shows_alert() {
    let mut engine = TorSettingsEngine::new(true, false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "new_circuit".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "New Circuit");
            assert!(!message.is_empty());
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

#[test]
fn tor_settings_new_circuit_disabled_when_tor_off() {
    let engine = TorSettingsEngine::new(false, false);
    let screen = engine.current_screen();

    let new_circuit_action = screen
        .actions
        .iter()
        .find(|a| a.id == "new_circuit")
        .expect("new_circuit action should exist");
    assert!(
        !new_circuit_action.enabled,
        "new_circuit should be disabled when tor is off"
    );
}

#[test]
fn tor_settings_new_circuit_enabled_when_tor_on() {
    let engine = TorSettingsEngine::new(true, false);
    let screen = engine.current_screen();

    let new_circuit_action = screen
        .actions
        .iter()
        .find(|a| a.id == "new_circuit")
        .expect("new_circuit action should exist");
    assert!(
        new_circuit_action.enabled,
        "new_circuit should be enabled when tor is on"
    );
}

#[test]
fn tor_settings_unknown_action_returns_update_screen() {
    let mut engine = TorSettingsEngine::new(false, false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "tor_settings");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// --- helpers ---

fn find_toggle(screen: &ScreenModel, list_id: &str, item_id: &str) -> bool {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == list_id => {
                items.iter().find(|t| t.id == item_id).map(|t| t.selected)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Toggle '{item_id}' not found in list '{list_id}'"))
}
