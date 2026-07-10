// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `SettingsEngine`.
//!
//! Single-screen settings list (`settings`) built entirely from
//! `SettingsGroup` toggles and dropdowns - all `SettingsToggled` /
//! dropdown / `ListItemSelected` pass-throughs. The engine renders
//! `actions: vec![]` (no standalone `ScreenAction`). The
//! `emergency_wipe` row opens an `InlineConfirm` behind the same
//! `screen_id` (BFS dedup collapses it); `confirm_emergency_wipe` /
//! `cancel_emergency_wipe` are covered by the engine's inline tests.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{DropdownOption, SettingsConfig, SettingsEngine, WorkflowEngine};

fn config() -> SettingsConfig {
    SettingsConfig {
        display_name: "Sample User".into(),
        delivery_receipts_enabled: true,
        suppress_presence: false,
        new_field_default_visible: false,
        contact_added_notifications: true,
        card_update_notifications: true,
        relay_url: "https://relay.test".into(),
        device_count: 1,
        password_set: false,
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
        reduce_motion: false,
        large_touch: false,
        show_help_icons: true,
        version: "0.0.0-test".into(),
        build: String::new(),
        pending_updates: 0,
        failed_deliveries: 0,
        debug_mode: false,
        backup_reminder_frequency: "Weekly".into(),
        last_backup_display: "Never".into(),
    }
}

// @internal
#[test]
fn settings_screen_is_reachable() {
    let engine = SettingsEngine::new(config());
    assert_eq!(engine.current_screen().screen_id, "settings");
    assert_reachability(&engine, &[]);
}
