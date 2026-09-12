// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-platform parity contract for the Settings screen.
//!
//! G7 of `2026-05-02-ios-humble-ui-deep-retirement`. Every shell renders
//! the same prepared Settings batch, so platform-level parity is
//! enforced as long as core's emitted shape stays stable. These tests
//! pin that shape: a canonical group list, in a fixed order, matching
//! what every renderer walks.
//!
//! Why pin order: the Settings screen is a scrollable surface;
//! re-ordering would change which groups appear above the fold on
//! every device. The 2026-05-08 device-test campaign (F-MED-3)
//! reported "Appearance unreachable on iOS" — root cause was a
//! reporter who stopped scrolling at the third group, which is
//! exactly the kind of regression a stable order makes visible
//! through review diffs rather than through user reports.

use vauchi_app::ui::{
    AppEngine, AppScreen, Component, DropdownOption, SettingsConfig, SettingsEngine,
    SettingsItemKind, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

/// The canonical SettingsGroup ids emitted by `SettingsEngine`, in the
/// order they appear on every renderer: the three design-canvas
/// sections. Everything the canvas does not show lives on the
/// Advanced, Appearance & Language and Accessibility sub-screens.
///
/// Editing this list is a cross-platform shape change — bump the list
/// intentionally and regenerate the screen catalog fixture in the same MR.
const EXPECTED_SETTINGS_GROUP_IDS: &[&str] = &["identity", "privacy", "app"];

const EXPECTED_ADVANCED_GROUP_IDS: &[&str] = &[
    "security", "backup", "network", "delivery", "about", "danger",
];

fn sample_settings_config() -> SettingsConfig {
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

fn group_ids(engine: &SettingsEngine) -> Vec<String> {
    engine
        .current_screen()
        .components
        .iter()
        .filter_map(|c| match c {
            Component::SettingsGroup { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

// @internal
#[test]
fn settings_screen_emits_full_group_set_in_stable_order() {
    let engine = SettingsEngine::new(sample_settings_config());
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "settings");
    assert_eq!(screen.title, "Settings");
    assert_eq!(
        group_ids(&engine),
        EXPECTED_SETTINGS_GROUP_IDS,
        "SettingsEngine emitted SettingsGroup ids do not match the cross-platform contract. \
         Editing the canonical list is intentional only when paired with a regenerated \
         screen catalog fixture in the same MR."
    );
}

// @internal
#[test]
fn relay_url_renders_as_link_so_renderers_emit_list_item_selected() {
    // Device regression 2026-06-10: `Value` rows are display-only in
    // every renderer, so the relay-URL editor was unreachable on mobile
    // even after the persistence fix. The row must be a `Link`
    // (tappable → `ListItemSelected("relay_url")` → intercept opens
    // `FormDialogType::EditRelayUrl`), carrying the current URL as its
    // detail text. Network lives on the Advanced sub-screen.
    let engine = SettingsEngine::new_advanced(sample_settings_config());
    let screen = engine.current_screen();

    let network_items = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SettingsGroup { id, items, .. } if id == "network" => Some(items),
            _ => None,
        })
        .expect("advanced settings screen must emit a network group");

    let item = network_items
        .iter()
        .find(|i| i.id == "relay_url")
        .expect("network group must contain the relay_url item");

    assert_eq!(
        item.kind,
        SettingsItemKind::Link {
            detail: Some("https://relay.test".into())
        },
        "relay_url must render as a tappable Link with the current URL as detail"
    );
}

// @internal
#[test]
fn appearance_screen_emits_theme_and_language_dropdowns() {
    let engine = SettingsEngine::new_appearance(sample_settings_config());
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
        "The Appearance & Language screen must surface Theme + Language as inline \
         Dropdowns; dropping either makes ADR-038 theme/language picks unreachable."
    );
    assert_eq!(group_ids(&engine), vec!["appearance"]);
}

// @internal
#[test]
fn advanced_screen_emits_its_group_set_with_danger_last() {
    let advanced = SettingsEngine::new_advanced(sample_settings_config());
    let advanced_json = serde_json::to_string(&advanced.current_screen())
        .expect("advanced settings screen must serialize");
    for required in ["\"danger\"", "\"emergency_wipe\""] {
        assert!(
            advanced_json.contains(required),
            "advanced Settings JSON missing `{required}`"
        );
    }
    assert_eq!(group_ids(&advanced), EXPECTED_ADVANCED_GROUP_IDS);
    assert_eq!(
        group_ids(&SettingsEngine::new_accessibility(sample_settings_config())),
        vec!["accessibility"]
    );
}

// @internal
#[test]
fn settings_advanced_link_navigates_to_advanced_subscreen() {
    let mut vauchi = vauchi_core::api::Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::Settings);

    let result = engine.handle_action(vauchi_app::ui::UserAction::ListItemSelected {
        component_id: "app".into(),
        item_id: "advanced".into(),
    });
    match result {
        vauchi_app::ui::ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "settings_advanced");
            assert_eq!(screen.parent_screen_id.as_deref(), Some("settings"));
        }
        other => panic!("expected NavigateTo(settings_advanced), got {other:?}"),
    }
}

// @internal
#[test]
fn settings_advanced_about_version_renders_non_empty_semver() {
    let mut vauchi = Vauchi::in_memory().expect("Vauchi::in_memory must succeed");
    vauchi.create_identity("Alice").expect("create_identity");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::SettingsAdvanced);
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
        "AppEngine-rendered Advanced settings screen must include the binding semver \
         `{pkg_version}`. If this fails, `app_engine/screens.rs` \
         `version: env!(\"CARGO_PKG_VERSION\").into()` was either dropped \
         (regressing to the F-005 empty-Version-row state) or the SettingsEngine \
         no longer renders the value. Fix at the construction site, not here. \
         Source: 2026-05-10 device-test campaign F-005."
    );
}

// @internal
#[test]
fn display_name_renders_as_link_so_renderers_emit_list_item_selected() {
    let engine = SettingsEngine::new(sample_settings_config());
    let screen = engine.current_screen();

    let identity_items = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SettingsGroup { id, items, .. } if id == "identity" => Some(items),
            _ => None,
        })
        .expect("settings screen must emit an identity group");

    let item = identity_items
        .iter()
        .find(|i| i.id == "display_name")
        .expect("identity group must contain the display_name item");

    assert_eq!(
        item.kind,
        SettingsItemKind::Link {
            detail: Some("Sample User".into())
        },
        "display_name must render as a tappable Link with the current name as detail"
    );
}

// @internal
#[test]
fn backup_reminders_renders_as_link_so_renderers_emit_list_item_selected() {
    let engine = SettingsEngine::new_advanced(sample_settings_config());
    let screen = engine.current_screen();

    let backup_items = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SettingsGroup { id, items, .. } if id == "backup" => Some(items),
            _ => None,
        })
        .expect("advanced settings screen must emit a backup group");

    let item = backup_items
        .iter()
        .find(|i| i.id == "backup_reminders")
        .expect("backup group must contain the backup_reminders item");

    assert_eq!(
        item.kind,
        SettingsItemKind::Link {
            detail: Some("Weekly".into())
        },
        "backup_reminders must render as a tappable Link with the current frequency as detail"
    );
}
