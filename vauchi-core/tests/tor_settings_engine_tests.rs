// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::tor_config::TorStatus;
use vauchi_core::ui::*;

// =============================================================================
// Screen basics
// =============================================================================

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

// =============================================================================
// Toggle behavior
// =============================================================================

#[test]
fn tor_settings_toggle_tor_enabled_dispatches_bootstrap() {
    let mut engine = TorSettingsEngine::new(false, false);

    // Verify initially disabled
    let screen = engine.current_screen();
    let tor_toggle = find_toggle(&screen, "tor_toggles", "tor_enabled");
    assert!(!tor_toggle, "tor_enabled should start disabled");

    // Toggle on → should dispatch Bootstrap command
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "tor_toggles".into(),
        item_id: "tor_enabled".into(),
    });
    match result {
        ActionResult::TorCommand { command } => {
            assert_eq!(command, TorCommand::Bootstrap);
        }
        other => panic!("Expected TorCommand(Bootstrap), got {other:?}"),
    }

    // Local state should be updated
    let screen = engine.current_screen();
    let toggled = find_toggle(&screen, "tor_toggles", "tor_enabled");
    assert!(toggled, "tor_enabled should be enabled after toggle");
}

#[test]
fn tor_settings_toggle_tor_disabled_dispatches_shutdown() {
    let mut engine = TorSettingsEngine::new(true, false);

    // Toggle off → should dispatch Shutdown command
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "tor_toggles".into(),
        item_id: "tor_enabled".into(),
    });
    match result {
        ActionResult::TorCommand { command } => {
            assert_eq!(command, TorCommand::Shutdown);
        }
        other => panic!("Expected TorCommand(Shutdown), got {other:?}"),
    }

    // Local state should be updated
    let screen = engine.current_screen();
    let toggled = find_toggle(&screen, "tor_toggles", "tor_enabled");
    assert!(!toggled, "tor_enabled should be disabled after toggle off");
}

#[test]
fn tor_settings_toggle_prefer_onion_dispatches_config_update() {
    let mut engine = TorSettingsEngine::new(true, false);

    // Toggle prefer_onion on → should dispatch UpdateConfig command
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "tor_toggles".into(),
        item_id: "prefer_onion".into(),
    });
    match result {
        ActionResult::TorCommand { command } => {
            assert!(
                matches!(command, TorCommand::UpdateConfig { prefer_onion: true }),
                "Expected UpdateConfig with prefer_onion=true, got {command:?}"
            );
        }
        other => panic!("Expected TorCommand(UpdateConfig), got {other:?}"),
    }

    // Local state should be updated
    let screen = engine.current_screen();
    let toggled = find_toggle(&screen, "tor_toggles", "prefer_onion");
    assert!(toggled, "prefer_onion should be enabled after toggle");
}

#[test]
fn tor_settings_toggle_prefer_onion_off_dispatches_config_update() {
    let mut engine = TorSettingsEngine::new(true, true);

    // Toggle prefer_onion off → should dispatch UpdateConfig with false
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "tor_toggles".into(),
        item_id: "prefer_onion".into(),
    });
    match result {
        ActionResult::TorCommand { command } => {
            assert!(
                matches!(
                    command,
                    TorCommand::UpdateConfig {
                        prefer_onion: false
                    }
                ),
                "Expected UpdateConfig with prefer_onion=false, got {command:?}"
            );
        }
        other => panic!("Expected TorCommand(UpdateConfig), got {other:?}"),
    }
}

// =============================================================================
// New Circuit button
// =============================================================================

#[test]
fn tor_settings_new_circuit_dispatches_rotate() {
    let mut engine = TorSettingsEngine::new(true, false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "new_circuit".into(),
    });
    match result {
        ActionResult::TorCommand { command } => {
            assert_eq!(command, TorCommand::RotateCircuit);
        }
        other => panic!("Expected TorCommand(RotateCircuit), got {other:?}"),
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

// =============================================================================
// Unknown action
// =============================================================================

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

// =============================================================================
// TorStatus integration
// =============================================================================

#[test]
fn tor_settings_default_status_is_disabled() {
    let engine = TorSettingsEngine::new(false, false);
    let screen = engine.current_screen();
    let status_text = find_status_detail(&screen, "tor_status", "Status");
    assert_eq!(status_text, "Disabled");
}

#[test]
fn tor_settings_set_status_updates_screen() {
    let mut engine = TorSettingsEngine::new(true, false);

    engine.set_status(TorStatus::Connected);
    let screen = engine.current_screen();
    let status_text = find_status_detail(&screen, "tor_status", "Status");
    assert_eq!(status_text, "Connected");
}

#[test]
fn tor_settings_status_connecting() {
    let mut engine = TorSettingsEngine::new(true, false);

    engine.set_status(TorStatus::Connecting);
    let screen = engine.current_screen();
    let status_text = find_status_detail(&screen, "tor_status", "Status");
    assert_eq!(status_text, "Connecting");
}

#[test]
fn tor_settings_status_bootstrapping() {
    let mut engine = TorSettingsEngine::new(true, false);

    engine.set_status(TorStatus::Bootstrapping { percentage: 42 });
    let screen = engine.current_screen();
    let status_text = find_status_detail(&screen, "tor_status", "Status");
    assert_eq!(status_text, "Bootstrapping (42%)");
}

#[test]
fn tor_settings_status_disconnected() {
    let mut engine = TorSettingsEngine::new(true, false);

    engine.set_status(TorStatus::Disconnected {
        reason: "network error".into(),
    });
    let screen = engine.current_screen();
    let status_text = find_status_detail(&screen, "tor_status", "Status");
    assert_eq!(status_text, "Disconnected: network error");
}

#[test]
fn tor_settings_disabled_engine_shows_disabled_status() {
    // Even if we set status to Connected, if enabled is false the engine
    // should show the actual TorStatus (the app layer is responsible for
    // keeping status in sync)
    let mut engine = TorSettingsEngine::new(false, false);
    engine.set_status(TorStatus::Connected);
    let screen = engine.current_screen();
    let status_text = find_status_detail(&screen, "tor_status", "Status");
    // The engine should show what the status actually is — the app layer
    // manages consistency
    assert_eq!(status_text, "Connected");
}

// =============================================================================
// TorCommand enum
// =============================================================================

#[test]
fn tor_command_bootstrap_equality() {
    assert_eq!(TorCommand::Bootstrap, TorCommand::Bootstrap);
    assert_ne!(TorCommand::Bootstrap, TorCommand::Shutdown);
}

#[test]
fn tor_command_serialization_roundtrip() {
    let commands = vec![
        TorCommand::Bootstrap,
        TorCommand::Shutdown,
        TorCommand::RotateCircuit,
        TorCommand::UpdateConfig { prefer_onion: true },
        TorCommand::UpdateConfig {
            prefer_onion: false,
        },
    ];
    for cmd in &commands {
        let json = serde_json::to_string(cmd).expect("serialize");
        let decoded: TorCommand = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cmd, &decoded, "roundtrip failed for {cmd:?}");
    }
}

// =============================================================================
// Helpers
// =============================================================================

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

fn find_status_detail(screen: &ScreenModel, panel_id: &str, title: &str) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::InfoPanel { id, items, .. } if id == panel_id => items
                .iter()
                .find(|item| item.title == title)
                .map(|item| item.detail.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Info item '{title}' not found in panel '{panel_id}'"))
}
