// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reachability test for `SettingsEngine`.
//!
//! Single-screen settings list (`settings`) built entirely from
//! `SettingsGroup` toggles and dropdowns - all `SettingsToggled` /
//! dropdown / `ListItemSelected` pass-throughs. The engine renders
//! `actions: vec![]` (no standalone `ScreenAction`). The
//! `emergency_wipe` row opens the shred screen (vauchi/private#598).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{DropdownOption, SettingsConfig, SettingsEngine, WorkflowEngine};

fn config() -> SettingsConfig {
    SettingsConfig {
        content_updates_in_background: true,
        display_name: "Sample User".into(),
        contact_added_notifications: true,
        card_update_notifications: true,
        relay_url: "https://relay.test".into(),
        device_count: 1,
        theme_id: "follow_system".into(),
        available_themes: vec![DropdownOption {
            id: "light".into(),
            label: "Light".into(),
        }],
        language_id: "follow_system".into(),
        available_languages: vec![DropdownOption {
            id: "en".into(),
            label: "English".into(),
        }],
        show_help_icons: true,
        version: "0.0.0-test".into(),
        backup_reminder_frequency: "Weekly".into(),
        ..Default::default()
    }
}

// @internal
#[test]
fn settings_screen_is_reachable() {
    let engine = SettingsEngine::new(config());
    assert_eq!(engine.current_screen().screen_id, "settings");
    assert_reachability(&engine, &[]);
}

// @internal
#[test]
fn settings_advanced_screen_is_reachable() {
    let engine = SettingsEngine::new_advanced(config());
    assert_eq!(engine.current_screen().screen_id, "settings_advanced");
    assert_reachability(&engine, &[]);
}

// @internal
#[test]
fn settings_appearance_screen_is_reachable() {
    let engine = SettingsEngine::new_appearance(config());
    assert_eq!(engine.current_screen().screen_id, "settings_appearance");
    assert_reachability(&engine, &[]);
}

// @internal
#[test]
fn settings_accessibility_screen_is_reachable() {
    let engine = SettingsEngine::new_accessibility(config());
    assert_eq!(engine.current_screen().screen_id, "settings_accessibility");
    assert_reachability(&engine, &[]);
}
