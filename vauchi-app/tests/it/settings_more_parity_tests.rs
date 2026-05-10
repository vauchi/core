// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-platform parity contract for the Settings + More screens.
//!
//! G7 of `2026-05-02-ios-humble-ui-deep-retirement`. Both iOS and
//! Android render the same `ScreenModel` JSON for `AppScreen::Settings`
//! and `AppScreen::More`, so platform-level parity is enforced as
//! long as core's emitted shape stays stable. These tests pin that
//! shape: a single canonical group/action list, in a fixed order,
//! matching what every Humble UI renderer must walk.
//!
//! Why pin order: the Settings screen is a long scrollable surface;
//! re-ordering would change which groups appear above the fold on
//! every device. The 2026-05-08 device-test campaign (F-MED-3)
//! reported "Appearance unreachable on iOS" — root cause was a
//! reporter who stopped scrolling at the third group, which is
//! exactly the kind of regression a stable order makes visible
//! through review diffs rather than through user reports.

use vauchi_app::ui::{
    Component, DropdownOption, MoreEngine, SettingsConfig, SettingsEngine, WorkflowEngine,
};

/// The canonical SettingsGroup ids emitted by `SettingsEngine`,
/// in the order they appear on every Humble UI renderer.
///
/// Editing this list is a cross-platform shape change — bump the
/// list intentionally and update both iOS + Android snapshot
/// expectations in the same MR.
const EXPECTED_SETTINGS_GROUP_IDS: &[&str] = &[
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
    "danger",
];

/// The canonical MoreEngine action_ids, in the order rendered by
/// every Humble UI renderer that hosts the "More" tab.
///
/// iOS currently hand-rolls its More tab (`ios/Vauchi/Views/MoreView.swift`)
/// instead of routing through `CoreScreenView(screenName: "More")`.
/// Pinning this list here means the day iOS adopts the unified
/// renderer (deferred per G1), the contract is already locked.
const EXPECTED_MORE_ACTION_IDS: &[&str] = &[
    "activity_log",
    "sync",
    "device_management",
    "device_replacement",
    "recovery",
    "archived_contacts",
    "contact_duplicates",
    "import_contacts",
    "settings",
    "backup",
    "privacy",
    "help",
];

fn sample_settings_config() -> SettingsConfig {
    SettingsConfig {
        display_name: "Sample User".into(),
        delivery_receipts_enabled: true,
        suppress_presence: false,
        contact_added_notifications: true,
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
        high_contrast: false,
        large_touch: false,
        show_help_icons: true,
        version: "0.0.0-test".into(),
        build: String::new(),
        sync_status: String::new(),
        pending_updates: 0,
        failed_deliveries: 0,
        debug_mode: false,
        backup_reminder_frequency: "Weekly".into(),
        last_backup_display: "Never".into(),
    }
}

// @internal
#[test]
fn settings_screen_emits_full_group_set_in_stable_order() {
    let engine = SettingsEngine::new(sample_settings_config());
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "settings");
    assert_eq!(screen.title, "Settings");

    let actual_group_ids: Vec<&str> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::SettingsGroup { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(
        actual_group_ids, EXPECTED_SETTINGS_GROUP_IDS,
        "SettingsEngine emitted SettingsGroup ids do not match the cross-platform contract. \
         Editing the canonical list is intentional only when paired with iOS + Android \
         renderer updates in the same MR."
    );
}

// @internal
#[test]
fn settings_screen_emits_theme_and_language_dropdowns() {
    let engine = SettingsEngine::new(sample_settings_config());
    let screen = engine.current_screen();

    let dropdown_ids: Vec<&str> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Dropdown { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(
        dropdown_ids,
        vec!["theme", "language"],
        "Settings screen must surface Theme + Language as inline Dropdowns. \
         Both iOS and Android render `Component::Dropdown` exhaustively, so \
         dropping either here makes ADR-038 theme/language picks unreachable \
         on every Humble UI frontend."
    );
}

// @internal
#[test]
fn settings_screen_appearance_and_danger_groups_present() {
    let engine = SettingsEngine::new(sample_settings_config());
    let screen = engine.current_screen();
    let json = serde_json::to_string(&screen).expect("settings screen must serialize");

    for required in ["\"appearance\"", "\"danger\"", "\"emergency_wipe\""] {
        assert!(
            json.contains(required),
            "Settings JSON missing `{required}` — would re-create the F-MED-3 \
             cross-platform divergence the device-test campaign flagged \
             on 2026-05-08."
        );
    }
}

// @internal
#[test]
fn more_screen_emits_full_action_set_in_stable_order() {
    let engine = MoreEngine::new();
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "more");
    assert_eq!(screen.title, "More");

    let action_list = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { items, .. } => Some(items),
            _ => None,
        })
        .expect("MoreEngine must emit a single Component::ActionList");

    let actual_action_ids: Vec<&str> = action_list.iter().map(|item| item.id.as_str()).collect();

    assert_eq!(
        actual_action_ids, EXPECTED_MORE_ACTION_IDS,
        "MoreEngine emitted action_ids do not match the cross-platform \
         contract. Android renders these via CoreScreenView; iOS will adopt \
         the same renderer when MoreView retires (deferred per G1 of \
         `2026-05-02-ios-humble-ui-deep-retirement`)."
    );
}
