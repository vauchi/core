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
    AppEngine, AppScreen, Component, DropdownOption, MoreEngine, SettingsConfig, SettingsEngine,
    WorkflowEngine,
};
use vauchi_core::api::Vauchi;

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

/// The canonical MoreEngine action_ids in render order — the
/// cumulative iteration across the four sections (primary →
/// secondary → data → legal). Mirrors the section grouping in
/// `core/vauchi-app/src/ui/more.rs`'s `MORE_SECTIONS`.
///
/// iOS currently hand-rolls its More tab (`ios/Vauchi/Views/MoreView.swift`)
/// with the same 3-section structure for the items it surfaces
/// (Settings, Help, Sync, Devices, Backup, Import, Privacy);
/// adopting `CoreScreenView(screenName: "More")` is a like-for-like
/// swap once G1 of `2026-05-02-ios-humble-ui-deep-retirement`
/// flips on. The extra Android/TUI items (Activity, Archived,
/// Merge, Replace, Backup-flat) live under the `secondary` / `data`
/// section ids; iOS surfaces a subset.
const EXPECTED_MORE_ACTION_IDS: &[&str] = &[
    // primary
    "settings",
    "help",
    // secondary
    "sync",
    "device_management",
    "device_replacement",
    "recovery",
    "backup",
    // data
    "archived_contacts",
    "contact_duplicates",
    "import_contacts",
    "activity_log",
    // legal
    "privacy",
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
fn settings_screen_about_version_renders_non_empty_semver() {
    // Regression for 2026-05-10 device-test campaign F-005:
    // `61b01b2b` (chore: bump core 0.50.0 → 0.51.0) landed at
    // 10:05 on 2026-05-08, and `5645efa2` (fix: render Settings
    // "Version" row with binding semver) landed 5h46m later at
    // 15:51. The published vauchi-platform 0.51.0 binding therefore
    // does not contain the fix, and every clean install of the
    // shipped app renders an empty Version row — re-verified
    // 2026-05-10 on Pixel 3a + Samsung S7 + iPhone SE clean
    // installs.
    //
    // The fix is now on core/main; this test guards against any
    // future refactor of `app_engine/screens.rs:238` silently
    // re-emptying `SettingsConfig.version`. We don't use
    // `sample_settings_config()` (which seeds a fake "0.0.0-test")
    // — the production construction site sets `env!("CARGO_PKG_VERSION")`
    // and we assert the rendered screen reflects that.
    // Drive the **production construction site** at
    // `app_engine/screens.rs:238`, not a synthetic SettingsConfig.
    // A test that builds its own SettingsConfig with a populated
    // `version` field would silently pass even if `screens.rs:238`
    // regressed back to `version: String::new()` — exactly the
    // regression shape we want to prevent.
    let mut vauchi = Vauchi::in_memory().expect("Vauchi::in_memory must succeed");
    vauchi.create_identity("Alice").expect("create_identity");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Settings);
    let screen = engine.current_screen();
    let json = serde_json::to_string(&screen).expect("settings screen must serialize");

    let pkg_version = env!("CARGO_PKG_VERSION");
    assert!(
        !pkg_version.is_empty(),
        "CARGO_PKG_VERSION must be non-empty at compile time — \
         vauchi-app's Cargo.toml is malformed."
    );
    assert!(
        json.contains(pkg_version),
        "AppEngine-rendered Settings screen must include the binding semver \
         `{pkg_version}`. If this fails, `app_engine/screens.rs:238` \
         `version: env!(\"CARGO_PKG_VERSION\").into()` was either dropped \
         (regressing to the F-005 empty-Version-row state) or the SettingsEngine \
         no longer renders the value. Fix at the construction site, not here. \
         Source: 2026-05-10 device-test campaign F-005."
    );
}

// @internal
#[test]
fn more_screen_emits_full_action_set_in_stable_order() {
    let engine = MoreEngine::new();
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "more");
    assert_eq!(screen.title, "More");

    let sections = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SectionedActionList { sections, .. } => Some(sections),
            _ => None,
        })
        .expect("MoreEngine must emit a single Component::SectionedActionList");

    let actual_action_ids: Vec<&str> = sections
        .iter()
        .flat_map(|sec| sec.items.iter())
        .map(|item| item.id.as_str())
        .collect();

    assert_eq!(
        actual_action_ids, EXPECTED_MORE_ACTION_IDS,
        "MoreEngine emitted action_ids (cumulative across sections) do not \
         match the cross-platform contract. Android renders these via \
         CoreScreenView; iOS will adopt the same renderer when MoreView \
         retires (deferred per G1 of \
         `2026-05-02-ios-humble-ui-deep-retirement`)."
    );
}

/// Section headers must describe their *contents*, not their priority
/// rank. "Primary"/"Secondary" are meaningless to a user (you cannot
/// guess what "Secondary" holds), so the headers are semantic. The two
/// backup/recovery entries must also be distinguishable: the entry that
/// opens the **Social Recovery** screen says exactly that — not a second
/// "Backup …" string sitting next to the file-`backup` entry, which read
/// as a confusing duplicate on device (2026-06-05-screen-ux-declutter).
// @internal
#[test]
fn more_section_headers_are_semantic_and_backup_recovery_disambiguated() {
    let engine = MoreEngine::new();
    let screen = engine.current_screen();
    let sections = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SectionedActionList { sections, .. } => Some(sections),
            _ => None,
        })
        .expect("MoreEngine must emit a single Component::SectionedActionList");

    let section_labels: Vec<&str> = sections.iter().map(|s| s.label.as_str()).collect();
    for banned in ["Primary", "Secondary"] {
        assert!(
            !section_labels.contains(&banned),
            "section headers must be semantic, not priority-rank — found `{banned}` in {section_labels:?}"
        );
    }

    let recovery_label = sections
        .iter()
        .flat_map(|s| s.items.iter())
        .find(|i| i.id == "recovery")
        .map(|i| i.label.as_str())
        .expect("`recovery` entry must exist");
    assert_eq!(
        recovery_label, "Social Recovery",
        "the entry opening the Social-Recovery screen must be labeled for it, \
         not a second `Backup & Recovery` string adjacent to the file-backup entry"
    );
}
