// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

fn sample_config() -> SettingsConfig {
    SettingsConfig {
        display_name: "Alice".into(),
        delivery_receipts_enabled: true,
        suppress_presence: false,
        relay_url: "wss://relay.vauchi.app".into(),
        device_count: 3,
        password_set: true,
    }
}

#[test]
fn settings_screen_id() {
    let engine = SettingsEngine::new(sample_config());
    assert_eq!(engine.current_screen().screen_id, "settings");
}

#[test]
fn settings_shows_all_groups() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();
    let groups: Vec<&str> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::SettingsGroup { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        groups,
        vec!["profile", "privacy", "security", "network", "danger"]
    );
}

#[test]
fn settings_toggle_delivery_receipts() {
    let mut engine = SettingsEngine::new(sample_config());

    // Initially enabled
    let screen = engine.current_screen();
    let receipts_enabled = find_toggle(&screen, "privacy", "delivery_receipts");
    assert!(receipts_enabled, "delivery_receipts should start enabled");

    // Toggle off
    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: "delivery_receipts".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let toggled = find_toggle(&screen, "privacy", "delivery_receipts");
            assert!(
                !toggled,
                "delivery_receipts should be disabled after toggle"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn settings_toggle_suppress_presence() {
    let mut engine = SettingsEngine::new(sample_config());

    // Initially disabled
    let screen = engine.current_screen();
    let suppress = find_toggle(&screen, "privacy", "suppress_presence");
    assert!(!suppress, "suppress_presence should start disabled");

    // Toggle on
    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: "suppress_presence".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let toggled = find_toggle(&screen, "privacy", "suppress_presence");
            assert!(toggled, "suppress_presence should be enabled after toggle");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn settings_reflects_config_values() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();

    let name_value = find_value(&screen, "profile", "display_name");
    assert_eq!(name_value, "Alice");

    let relay_value = find_value(&screen, "network", "relay_url");
    assert_eq!(relay_value, "wss://relay.vauchi.app");
}

#[test]
fn settings_emergency_wipe_shows_alert() {
    let mut engine = SettingsEngine::new(sample_config());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Emergency Wipe");
            assert_eq!(message, "This will permanently delete all data.");
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

#[test]
fn settings_device_count_in_detail() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();

    let detail = find_link_detail(&screen, "security", "devices");
    assert_eq!(detail.as_deref(), Some("3 device(s)"));
}

// --- helpers ---

fn find_settings_group<'a>(screen: &'a ScreenModel, group_id: &str) -> &'a [SettingsItem] {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SettingsGroup { id, items, .. } if id == group_id => Some(items.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("SettingsGroup '{group_id}' not found"))
}

fn find_toggle(screen: &ScreenModel, group_id: &str, item_id: &str) -> bool {
    let items = find_settings_group(screen, group_id);
    items
        .iter()
        .find_map(|item| match &item.kind {
            SettingsItemKind::Toggle { enabled } if item.id == item_id => Some(*enabled),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Toggle '{item_id}' not found in group '{group_id}'"))
}

fn find_value(screen: &ScreenModel, group_id: &str, item_id: &str) -> String {
    let items = find_settings_group(screen, group_id);
    items
        .iter()
        .find_map(|item| match &item.kind {
            SettingsItemKind::Value { value } if item.id == item_id => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Value '{item_id}' not found in group '{group_id}'"))
}

fn find_link_detail(screen: &ScreenModel, group_id: &str, item_id: &str) -> Option<String> {
    let items = find_settings_group(screen, group_id);
    items
        .iter()
        .find_map(|item| match &item.kind {
            SettingsItemKind::Link { detail } if item.id == item_id => Some(detail.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Link '{item_id}' not found in group '{group_id}'"))
}
