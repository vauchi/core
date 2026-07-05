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
        theme_id: String::new(),
        available_themes: vec![],
        language_id: String::new(),
        available_languages: vec![],
        reduce_motion: false,
        high_contrast: false,
        large_touch: false,
        show_help_icons: true,
        version: String::new(),
        build: String::new(),
        pending_updates: 0,
        failed_deliveries: 0,
        debug_mode: false,
        backup_reminder_frequency: "Weekly".into(),
        last_backup_display: "Never".into(),
    }
}

// @internal
// @internal
#[test]
fn settings_screen_id() {
    let engine = SettingsEngine::new(sample_config());
    assert_eq!(engine.current_screen().screen_id, "settings");
}

// @internal
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
// @internal
#[test]
fn settings_toggle_delivery_receipts() {
    let mut engine = SettingsEngine::new(sample_config());

    let screen = engine.current_screen();
    let receipts_enabled = find_toggle(&screen, "privacy", "delivery_receipts");
    assert!(receipts_enabled, "delivery_receipts should start enabled");

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
// @internal
#[test]
fn settings_toggle_suppress_presence() {
    let mut engine = SettingsEngine::new(sample_config());

    let screen = engine.current_screen();
    let suppress = find_toggle(&screen, "privacy", "suppress_presence");
    assert!(!suppress, "suppress_presence should start disabled");

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
// @internal
#[test]
fn settings_reflects_config_values() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();

    // display_name is now a tappable Link (carries the name as detail) so its
    // rename handler is reachable — see settings_more_parity_tests.
    let name_detail = find_link_detail(&screen, "profile", "display_name");
    assert_eq!(name_detail.as_deref(), Some("Alice"));

    let relay_detail = find_link_detail(&screen, "network", "relay_url");
    assert_eq!(relay_detail.as_deref(), Some("https://relay.vauchi.app"));
}

// ADR-022: irrevocable actions use InlineConfirm, not ShowAlert
// @internal
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
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_emergency_wipe".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "confirm_emergency_wipe should return Complete, got {result:?}"
    );
}

// @internal
// @internal
#[test]
fn settings_cancel_emergency_wipe_removes_inline_confirm() {
    let mut engine = SettingsEngine::new(sample_config());
    let trigger = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    assert!(
        matches!(trigger, ActionResult::UpdateScreen(_)),
        "trigger should show inline confirm, got {trigger:?}"
    );
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

// @internal
#[test]
fn settings_device_count_in_detail() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();

    let detail = find_link_detail(&screen, "security", "devices");
    assert_eq!(detail.as_deref(), Some("3 devices"));
}

// @internal
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
fn settings_emits_theme_and_language_dropdowns() {
    use vauchi_app::ui::{Component, DropdownOption};
    let mut config = sample_config();
    config.theme_id = "catppuccin-mocha".into();
    config.language_id = "en".into();
    config.available_themes = vec![
        DropdownOption {
            id: "catppuccin-mocha".into(),
            label: "Catppuccin Mocha".into(),
        },
        DropdownOption {
            id: "catppuccin-latte".into(),
            label: "Catppuccin Latte".into(),
        },
    ];
    config.available_languages = vec![DropdownOption {
        id: "en".into(),
        label: "English".into(),
    }];
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();

    let theme_dropdown = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown {
                id,
                selected,
                options,
                ..
            } if id == "theme" => Some((selected.clone(), options.clone())),
            _ => None,
        })
        .expect("theme Dropdown component");
    // Contract: Component::Dropdown.selected carries the option id, not the label.
    // Frontends match `selected` against `options[i].id` to highlight the active choice.
    assert_eq!(theme_dropdown.0.as_deref(), Some("catppuccin-mocha"));
    assert_eq!(theme_dropdown.1[0].id, "follow_system");
    assert_eq!(theme_dropdown.1[0].label, "System");
    assert_eq!(theme_dropdown.1[1].id, "catppuccin-mocha");
    assert_eq!(theme_dropdown.1[2].id, "catppuccin-latte");

    let language_dropdown = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown {
                id,
                selected,
                options,
                ..
            } if id == "language" => Some((selected.clone(), options.clone())),
            _ => None,
        })
        .expect("language Dropdown component");
    assert_eq!(language_dropdown.0.as_deref(), Some("en"));
    assert_eq!(language_dropdown.1[0].id, "follow_system");
    assert_eq!(language_dropdown.1[1].id, "en");
}

// @internal
#[test]
fn settings_theme_dropdown_selection_stores_id() {
    use vauchi_app::ui::{Component, DropdownOption};
    let mut config = sample_config();
    config.theme_id = "follow_system".into();
    config.available_themes = vec![DropdownOption {
        id: "ocean-dark".into(),
        label: "Ocean Dark".into(),
    }];
    let mut engine = SettingsEngine::new(config);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "theme".into(),
        item_id: "ocean-dark".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("expected UpdateScreen, got {result:?}");
    };
    let selected = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, selected, .. } if id == "theme" => selected.clone(),
            _ => None,
        })
        .expect("theme Dropdown selected");
    assert_eq!(selected, "ocean-dark");
}

// @internal
#[test]
fn settings_theme_dropdown_follow_system_resets_to_reserved_id() {
    use vauchi_app::ui::{Component, DropdownOption};
    let mut config = sample_config();
    config.theme_id = "ocean-dark".into();
    config.available_themes = vec![DropdownOption {
        id: "ocean-dark".into(),
        label: "Ocean Dark".into(),
    }];
    let mut engine = SettingsEngine::new(config);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "theme".into(),
        item_id: "follow_system".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("expected UpdateScreen, got {result:?}");
    };
    let selected = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, selected, .. } if id == "theme" => selected.clone(),
            _ => None,
        })
        .expect("theme Dropdown selected");
    assert_eq!(selected, "follow_system");
}

// @internal
#[test]
fn settings_language_dropdown_selection_stores_id() {
    use vauchi_app::ui::{Component, DropdownOption};
    let mut config = sample_config();
    config.language_id = "follow_system".into();
    config.available_languages = vec![DropdownOption {
        id: "en".into(),
        label: "English".into(),
    }];
    let mut engine = SettingsEngine::new(config);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "language".into(),
        item_id: "en".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("expected UpdateScreen, got {result:?}");
    };
    let selected = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, selected, .. } if id == "language" => selected.clone(),
            _ => None,
        })
        .expect("language Dropdown selected");
    assert_eq!(selected, "en");
}

// @internal
// @internal
#[test]
fn settings_accessibility_toggles() {
    let mut engine = SettingsEngine::new(sample_config());

    let screen = engine.current_screen();
    assert!(!find_toggle(&screen, "accessibility", "reduce_motion"));
    assert!(!find_toggle(&screen, "accessibility", "high_contrast"));
    assert!(!find_toggle(&screen, "accessibility", "large_touch"));

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
// @internal
#[test]
fn settings_delivery_section() {
    let mut config = sample_config();
    config.pending_updates = 3;
    config.failed_deliveries = 1;
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();

    let pending = find_value(&screen, "delivery", "pending_updates");
    assert_eq!(pending, "3");
    // Failed Deliveries is a Link into the DeliveryStatus screen (M4 S2 —
    // the standalone Sync screen + its "Sync Status" row were retired); its
    // detail carries the live failed count.
    let failed = find_link_detail(&screen, "delivery", "failed_deliveries");
    assert_eq!(failed.as_deref(), Some("1"));
}

// @internal
// @internal
#[test]
fn settings_backup_section_has_links() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();
    let items = find_settings_group(&screen, "backup");
    assert_eq!(items.len(), 5);
    assert!(matches!(items[0].kind, SettingsItemKind::Link { .. }));
    assert!(matches!(items[1].kind, SettingsItemKind::Link { .. }));
    assert!(matches!(items[2].kind, SettingsItemKind::Link { .. }));
    assert_eq!(items[2].id, "setup_new_device");
    assert!(matches!(items[3].kind, SettingsItemKind::Value { .. }));
    assert_eq!(items[3].id, "last_backup");
    // backup_reminders is a Link (tappable, cycles frequency) — it was a Value
    // that orphaned its handler (2026-04-06-display-name-rename-fails sibling).
    // last_backup above stays a Value: display-only, no handler.
    assert!(matches!(items[4].kind, SettingsItemKind::Link { .. }));
    assert_eq!(items[4].id, "backup_reminders");
}

// @internal
// @internal
#[test]
fn settings_items_have_a11y_labels() {
    let mut engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();

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

// @internal
#[test]
fn settings_show_help_icons_appears_in_appearance_group() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();
    // Verify the toggle is present and defaults to true
    let enabled = find_toggle(&screen, "appearance", "show_help_icons");
    assert!(enabled, "show_help_icons should default to true");
}

// @internal
#[test]
fn settings_show_help_icons_toggle_flips_config() {
    let mut engine = SettingsEngine::new(sample_config());

    let screen = engine.current_screen();
    assert!(
        find_toggle(&screen, "appearance", "show_help_icons"),
        "show_help_icons should start enabled"
    );

    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "appearance".into(),
        item_id: "show_help_icons".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("Expected UpdateScreen, got {result:?}");
    };
    assert!(
        !find_toggle(&screen, "appearance", "show_help_icons"),
        "show_help_icons should be disabled after toggle"
    );

    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "appearance".into(),
        item_id: "show_help_icons".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("Expected UpdateScreen, got {result:?}");
    };
    assert!(
        find_toggle(&screen, "appearance", "show_help_icons"),
        "show_help_icons should be re-enabled after second toggle"
    );
}

// @internal
#[test]
fn settings_duress_pin_has_info_key_when_help_icons_enabled() {
    let mut config = sample_config();
    config.show_help_icons = true;
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();
    let items = find_settings_group(&screen, "security");
    let duress = items
        .iter()
        .find(|i| i.id == "duress_pin")
        .expect("duress_pin not found");
    assert_eq!(
        duress.info_key.as_deref(),
        Some("duress_pin"),
        "duress_pin should have info_key when show_help_icons is true"
    );
}

// @internal
#[test]
fn settings_duress_pin_has_no_info_key_when_help_icons_disabled() {
    let mut config = sample_config();
    config.show_help_icons = false;
    let engine = SettingsEngine::new(config);
    let screen = engine.current_screen();
    let items = find_settings_group(&screen, "security");
    let duress = items
        .iter()
        .find(|i| i.id == "duress_pin")
        .expect("duress_pin not found");
    assert_eq!(
        duress.info_key, None,
        "duress_pin should have no info_key when show_help_icons is false"
    );
}

// @internal
#[test]
fn settings_other_items_have_no_info_key() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();
    for component in &screen.components {
        if let Component::SettingsGroup { id, items, .. } = component {
            for item in items {
                if item.id != "duress_pin" {
                    assert_eq!(
                        item.info_key, None,
                        "item '{}' in group '{}' should have no info_key",
                        item.id, id
                    );
                }
            }
        }
    }
}

// @internal
#[test]
fn settings_about_has_what_is_vauchi_item() {
    let engine = SettingsEngine::new(sample_config());
    let screen = engine.current_screen();
    let items = find_settings_group(&screen, "about");
    let item = items.iter().find(|i| i.id == "what_is_vauchi");
    assert!(
        item.is_some(),
        "about group should contain what_is_vauchi item"
    );
    let item = item.unwrap();
    assert_eq!(item.label, "What is Vauchi?");
    assert!(matches!(item.kind, SettingsItemKind::Link { .. }));
}

// @internal
#[test]
fn settings_select_what_is_vauchi_returns_show_info_overlay() {
    let mut engine = SettingsEngine::new(sample_config());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "about".into(),
        item_id: "what_is_vauchi".into(),
    });
    match result {
        ActionResult::ShowInfoOverlay { title, body } => {
            assert_eq!(title, "What is Vauchi?");
            assert!(!body.is_empty(), "body should not be empty");
            assert!(
                !body.starts_with("Missing:"),
                "body should not be a missing-key placeholder"
            );
        }
        other => panic!("Expected ShowInfoOverlay, got {other:?}"),
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
