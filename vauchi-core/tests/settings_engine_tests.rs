// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn sample_config() -> SettingsConfig {
    SettingsConfig {
        display_name: "Alice".into(),
        delivery_receipts_enabled: true,
        suppress_presence: false,
        contact_added_notifications: false,
        relay_url: "https://relay.vauchi.app".into(),
        device_count: 3,
        password_set: true,
        theme: String::new(),
        available_themes: vec![],
        language: String::new(),
        available_languages: vec![],
        reduce_motion: false,
        high_contrast: false,
        large_touch: false,
        version: String::new(),
        build: String::new(),
        sync_status: String::new(),
        pending_updates: 0,
        failed_deliveries: 0,
        debug_mode: false,
    }
}

// @internal
#[test]
fn settings_screen_id() {
    let engine = SettingsEngine::new(sample_config());
    assert_eq!(engine.current_screen().screen_id, "settings");
}

// @internal
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
        vec![
            "profile",
            "privacy",
            "notifications",
            "appearance",
            "accessibility",
            "security",
            "backup",
            "network",
            "delivery",
            "help",
            "about",
            "danger"
        ]
    );
}

// @internal
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

// @internal
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

// @internal
#[test]
fn settings_reflects_config_values() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();

    let name_value = find_value(&screen, "profile", "display_name");
    assert_eq!(name_value, "Alice");

    let relay_value = find_value(&screen, "network", "relay_url");
    assert_eq!(relay_value, "https://relay.vauchi.app");
}

// ADR-022: irrevocable actions use InlineConfirm, not ShowAlert
// @internal
#[test]
fn settings_emergency_wipe_shows_inline_confirm() {
    let mut engine = SettingsEngine::new(sample_config());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("Expected UpdateScreen with InlineConfirm, got {result:?}");
    };
    let has_inline_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { destructive, .. } if *destructive));
    assert!(
        has_inline_confirm,
        "emergency_wipe should show a destructive InlineConfirm"
    );
}

// @internal
#[test]
fn settings_confirm_emergency_wipe_completes() {
    let mut engine = SettingsEngine::new(sample_config());
    // Trigger wipe to enter pending state
    let trigger = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    assert!(
        matches!(trigger, ActionResult::UpdateScreen(_)),
        "trigger should show inline confirm, got {trigger:?}"
    );
    // Confirm
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_emergency_wipe".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "confirm_emergency_wipe should return Complete, got {result:?}"
    );
}

// @internal
#[test]
fn settings_cancel_emergency_wipe_removes_inline_confirm() {
    let mut engine = SettingsEngine::new(sample_config());
    // Trigger wipe
    let trigger = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    assert!(
        matches!(trigger, ActionResult::UpdateScreen(_)),
        "trigger should show inline confirm, got {trigger:?}"
    );
    // Cancel
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_emergency_wipe".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("Expected UpdateScreen, got {result:?}");
    };
    let has_inline_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { .. }));
    assert!(!has_inline_confirm, "cancel should remove InlineConfirm");
}

#[test]
fn settings_device_count_in_detail() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();

    let detail = find_link_detail(&screen, "security", "devices");
    assert_eq!(detail.as_deref(), Some("3 devices"));
}

// @internal
#[test]
fn settings_single_device_no_plural() {
    let mut config = sample_config();
    config.device_count = 1;
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();
    let detail = find_link_detail(&screen, "security", "devices");
    assert_eq!(detail.as_deref(), Some("1 device"));
}

// @internal
#[test]
fn settings_appearance_section_has_theme_and_language() {
    let mut config = sample_config();
    config.theme = "dark".into();
    config.language = "English".into();
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();
    let theme = find_value(&screen, "appearance", "theme");
    assert_eq!(theme, "dark");
    let lang = find_value(&screen, "appearance", "language");
    assert_eq!(lang, "English");
}

// @internal
#[test]
fn settings_accessibility_toggles() {
    let mut engine = SettingsEngine::new(sample_config());

    // Initially all false
    let screen = engine.current_screen();
    assert!(!find_toggle(&screen, "accessibility", "reduce_motion"));
    assert!(!find_toggle(&screen, "accessibility", "high_contrast"));
    assert!(!find_toggle(&screen, "accessibility", "large_touch"));

    // Toggle reduce_motion
    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "accessibility".into(),
        item_id: "reduce_motion".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!()
    };
    assert!(find_toggle(&screen, "accessibility", "reduce_motion"));
}

// @internal
#[test]
fn settings_about_shows_version() {
    let mut config = sample_config();
    config.version = "0.19.0".into();
    config.build = "42".into();
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();
    let version = find_value(&screen, "about", "version");
    assert_eq!(version, "0.19.0 (42)");
}

// @internal
#[test]
fn settings_about_version_without_build() {
    let mut config = sample_config();
    config.version = "0.19.0".into();
    config.build = String::new();
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();
    let version = find_value(&screen, "about", "version");
    assert_eq!(version, "0.19.0");
}

// @internal
#[test]
fn settings_debug_mode_toggle() {
    let mut engine = SettingsEngine::new(sample_config());
    assert!(!find_toggle(
        &engine.current_screen(),
        "about",
        "debug_mode"
    ));

    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "about".into(),
        item_id: "debug_mode".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!()
    };
    assert!(find_toggle(&screen, "about", "debug_mode"));
}

// @internal
#[test]
fn settings_delivery_section() {
    let mut config = sample_config();
    config.sync_status = "Connected".into();
    config.pending_updates = 3;
    config.failed_deliveries = 1;
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();

    let sync = find_link_detail(&screen, "delivery", "sync");
    assert_eq!(sync.as_deref(), Some("Connected"));
    let pending = find_value(&screen, "delivery", "pending_updates");
    assert_eq!(pending, "3");
    let failed = find_value(&screen, "delivery", "failed_deliveries");
    assert_eq!(failed, "1");
}

// @internal
#[test]
fn settings_backup_section_has_links() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();
    let items = find_settings_group(&screen, "backup");
    assert_eq!(items.len(), 3);
    assert!(matches!(items[0].kind, SettingsItemKind::Link { .. }));
    assert!(matches!(items[1].kind, SettingsItemKind::Link { .. }));
    assert!(matches!(items[2].kind, SettingsItemKind::Link { .. }));
    assert_eq!(items[2].id, "setup_new_device");
}

// @internal
#[test]
fn settings_items_have_a11y_labels() {
    let mut engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();

    // Check every SettingsGroup's items have a11y
    for component in &screen.components {
        if let Component::SettingsGroup { id, items, .. } = component {
            for item in items {
                assert!(
                    item.a11y.is_some(),
                    "SettingsItem '{}' in group '{}' missing a11y label",
                    item.id,
                    id
                );
            }
        }
    }

    // Also check InlineConfirm a11y when pending_wipe is active
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    let wipe_screen = engine.current_screen();
    for component in &wipe_screen.components {
        if let Component::InlineConfirm { id, a11y, .. } = component {
            assert!(a11y.is_some(), "InlineConfirm '{}' missing a11y label", id);
        }
    }
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
