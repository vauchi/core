// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Settings batch matches the design canvas: three sections (My
//! identity / Privacy / App) with one-line row descriptions, and the
//! rows the canvas does not show kept reachable on the Advanced,
//! Appearance & Language and Accessibility sub-screens (ADR-066: the
//! shell renders what Core prepares, so the grouping is Core's).

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, PreparedSurface, SettingsConfig, SettingsEngine,
    SettingsItem, SettingsItemKind, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, PresentationNode, SurfaceId};

fn config() -> SettingsConfig {
    SettingsConfig {
        display_name: "Sample User".into(),
        device_count: 1,
        last_backup_display: "Never".into(),
        ..Default::default()
    }
}

fn config_in(language_id: &str) -> SettingsConfig {
    SettingsConfig {
        language_id: language_id.into(),
        ..config()
    }
}

fn groups(components: &[Component]) -> Vec<(&str, &str, &[SettingsItem])> {
    components
        .iter()
        .filter_map(|c| match c {
            Component::SettingsGroup { id, label, items } => {
                Some((id.as_str(), label.as_str(), items.as_slice()))
            }
            _ => None,
        })
        .collect()
}

fn group<'a>(components: &'a [Component], id: &str) -> (&'a str, &'a [SettingsItem]) {
    groups(components)
        .into_iter()
        .find(|(gid, _, _)| *gid == id)
        .map(|(_, label, items)| (label, items))
        .unwrap_or_else(|| panic!("no `{id}` group; components: {components:?}"))
}

fn row<'a>(items: &'a [SettingsItem], id: &str) -> &'a SettingsItem {
    items
        .iter()
        .find(|item| item.id == id)
        .unwrap_or_else(|| panic!("no `{id}` row in {items:?}"))
}

fn ids(items: &[SettingsItem]) -> Vec<&str> {
    items.iter().map(|item| item.id.as_str()).collect()
}

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn settings_main_screen_is_the_three_canvas_sections() {
    let screen = SettingsEngine::new(config()).current_screen();

    let sections: Vec<(&str, &str)> = groups(&screen.components)
        .into_iter()
        .map(|(id, label, _)| (id, label))
        .collect();
    assert_eq!(
        sections,
        vec![
            ("identity", "My identity"),
            ("privacy", "Privacy"),
            ("app", "App"),
        ]
    );
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::Dropdown { .. })),
        "theme + language dropdowns belong to the Appearance & Language sub-screen"
    );
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn identity_section_lists_the_canvas_rows_with_their_descriptions() {
    let screen = SettingsEngine::new(config()).current_screen();
    let (_, items) = group(&screen.components, "identity");

    let rows: Vec<(&str, &str, Option<&str>)> = items
        .iter()
        .map(|item| {
            (
                item.id.as_str(),
                item.label.as_str(),
                item.subtitle.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("display_name", "Display Name", None),
            ("edit_profile", "My Contact Info", None),
            ("devices", "My Devices", None),
            ("backup_export", "Backup", Some("Last backup: Never")),
            (
                "recovery",
                "Recovery",
                Some("Only if you lose every device")
            ),
        ]
    );
    assert_eq!(
        row(items, "display_name").kind,
        SettingsItemKind::Link {
            detail: Some("Sample User".into())
        }
    );
    assert_eq!(
        row(items, "devices").kind,
        SettingsItemKind::Link {
            detail: Some("1 device".into())
        }
    );
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn privacy_section_keeps_the_toggles_next_to_your_data() {
    let screen = SettingsEngine::new(config()).current_screen();
    let (_, items) = group(&screen.components, "privacy");

    assert_eq!(
        ids(items),
        vec![
            "privacy",
            "suppress_presence",
            "delivery_receipts",
            "new_field_default",
            "card_update",
            "contact_added",
            "activity_log",
        ]
    );
    let your_data = row(items, "privacy");
    assert_eq!(your_data.label, "Your data");
    assert_eq!(
        your_data.subtitle.as_deref(),
        Some("What is stored, and where")
    );
    assert_eq!(your_data.kind, SettingsItemKind::Link { detail: None });

    let presence = row(items, "suppress_presence");
    assert_eq!(presence.label, "Suppress Presence");
    assert_eq!(
        presence.subtitle.as_deref(),
        Some("When enabled, hides your online status from contacts")
    );
    assert_eq!(presence.kind, SettingsItemKind::Toggle { enabled: false });
    for toggle in [
        "delivery_receipts",
        "new_field_default",
        "card_update",
        "contact_added",
    ] {
        assert!(
            matches!(row(items, toggle).kind, SettingsItemKind::Toggle { .. }),
            "{toggle} stays a toggle"
        );
    }
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn app_section_routes_to_appearance_accessibility_help_and_advanced() {
    let screen = SettingsEngine::new(config()).current_screen();
    let (_, items) = group(&screen.components, "app");

    let rows: Vec<(&str, &str, Option<&str>)> = items
        .iter()
        .map(|item| {
            (
                item.id.as_str(),
                item.label.as_str(),
                item.subtitle.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("appearance", "Appearance & Language", None),
            (
                "accessibility",
                "Accessibility",
                Some("Large text, high contrast, reduce motion")
            ),
            ("help_center", "Help", None),
            ("advanced", "Advanced", None),
        ]
    );
    for item in items {
        assert_eq!(
            item.kind,
            SettingsItemKind::Link { detail: None },
            "{}",
            item.id
        );
    }
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn appearance_sub_screen_carries_theme_language_and_help_icons() {
    let screen = SettingsEngine::new_appearance(config()).current_screen();

    assert_eq!(screen.screen_id, "settings_appearance");
    assert_eq!(screen.title, "Appearance & Language");
    let dropdowns: Vec<&str> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Dropdown { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(dropdowns, vec!["theme", "language"]);
    let (_, items) = group(&screen.components, "appearance");
    assert_eq!(ids(items), vec!["show_help_icons"]);
    assert_eq!(
        row(items, "show_help_icons").kind,
        SettingsItemKind::Toggle { enabled: true }
    );
}

// @scenario: accessibility :: Accessibility toggles live on their own sub-screen
#[test]
fn accessibility_sub_screen_carries_the_motion_and_touch_toggles() {
    let screen = SettingsEngine::new_accessibility(config()).current_screen();

    assert_eq!(screen.screen_id, "settings_accessibility");
    assert_eq!(screen.title, "Accessibility");
    let (_, items) = group(&screen.components, "accessibility");
    assert_eq!(ids(items), vec!["reduce_motion", "large_touch"]);
    for item in items {
        assert_eq!(
            item.kind,
            SettingsItemKind::Toggle { enabled: false },
            "{}",
            item.id
        );
    }
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn advanced_screen_keeps_every_row_the_canvas_dropped() {
    let screen = SettingsEngine::new_advanced(config()).current_screen();

    let sections: Vec<&str> = groups(&screen.components)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(
        sections,
        vec![
            "security", "backup", "network", "delivery", "about", "danger"
        ],
        "danger stays last, far from the thumb"
    );
    let (label, security) = group(&screen.components, "security");
    assert_eq!(label, "Security");
    assert_eq!(
        ids(security),
        vec![
            "change_password",
            "duress_pin",
            "decoy_contacts",
            "setup_new_device"
        ]
    );
    let (label, backup) = group(&screen.components, "backup");
    assert_eq!(label, "Backup");
    assert_eq!(ids(backup), vec!["backup_reminders"]);
    let (label, about) = group(&screen.components, "about");
    assert_eq!(label, "About");
    assert_eq!(
        ids(about),
        vec![
            "what_is_vauchi",
            "funding",
            "support",
            "version",
            "debug_mode"
        ]
    );
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn a_privacy_toggle_flips_in_place_on_the_main_screen() {
    let mut engine = SettingsEngine::new(config());

    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: "suppress_presence".into(),
    });

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("a toggle re-renders in place, got {result:?}");
    };
    let (_, items) = group(&screen.components, "privacy");
    assert_eq!(
        row(items, "suppress_presence").kind,
        SettingsItemKind::Toggle { enabled: true }
    );
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn appearance_and_accessibility_rows_open_their_sub_screens() {
    for (item_id, screen_id, app_screen) in [
        (
            "appearance",
            "settings_appearance",
            AppScreen::SettingsAppearance,
        ),
        (
            "accessibility",
            "settings_accessibility",
            AppScreen::SettingsAccessibility,
        ),
    ] {
        let mut engine = engine_with_identity();
        engine.navigate_to(AppScreen::Settings);

        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "app".into(),
            item_id: item_id.into(),
        });

        let ActionResult::NavigateTo(screen) = result else {
            panic!("`{item_id}` must navigate, got {result:?}");
        };
        assert_eq!(screen.screen_id, screen_id);
        assert_eq!(screen.parent_screen_id.as_deref(), Some("settings"));
        assert_eq!(*engine.current_app_screen(), app_screen);
        assert_eq!(AppScreen::from_screen_id(screen_id), Some(app_screen));
    }
}

// @scenario: accessibility :: Accessibility toggles live on their own sub-screen
#[test]
fn accessibility_toggle_on_its_sub_screen_persists_to_config() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::SettingsAccessibility);
    assert!(!engine.vauchi().config().reduce_motion);

    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "accessibility".into(),
        item_id: "reduce_motion".into(),
    });

    assert!(engine.vauchi().config().reduce_motion);
    assert!(engine.vauchi().load_settings_flags().unwrap().reduce_motion);
}

// @scenario: navigation :: Settings groups follow the design canvas
#[test]
fn row_descriptions_ride_the_prepared_row_subtitle() {
    let screen = SettingsEngine::new(config()).current_screen();
    let surface = PreparedSurface::from_screen(SurfaceId::new("settings").unwrap(), 1, &screen)
        .expect("settings projects");
    let Command::ReplaceSurface { surface } = surface.command() else {
        panic!("expected ReplaceSurface");
    };

    let subtitles: Vec<(String, Option<String>)> = surface
        .nodes
        .iter()
        .filter_map(|node| match node {
            PresentationNode::List { rows, .. } => Some(rows),
            _ => None,
        })
        .flatten()
        .map(|row| (row.title.clone(), row.subtitle.clone()))
        .collect();

    assert!(subtitles.contains(&(
        "Recovery".into(),
        Some("Only if you lose every device".into())
    )));
    assert!(subtitles.contains(&(
        "Accessibility".into(),
        Some("Large text, high contrast, reduce motion".into())
    )));
    assert!(subtitles.contains(&("My Devices".into(), None)));
}

// @scenario: internationalization :: Settings sections render the active locale
#[test]
fn settings_canvas_copy_renders_the_active_locale() {
    load_german();
    let de = SettingsEngine::new(config_in("de")).current_screen();
    let en = SettingsEngine::new(config_in("")).current_screen();

    let (de_identity, de_items) = group(&de.components, "identity");
    let (en_identity, en_items) = group(&en.components, "identity");
    assert_translated("identity section label", de_identity, en_identity);
    assert_translated(
        "recovery description",
        row(de_items, "recovery").subtitle.as_deref().unwrap(),
        row(en_items, "recovery").subtitle.as_deref().unwrap(),
    );
    assert_translated(
        "my contact info row",
        &row(de_items, "edit_profile").label,
        &row(en_items, "edit_profile").label,
    );

    let (_, de_app) = group(&de.components, "app");
    let (_, en_app) = group(&en.components, "app");
    assert_translated(
        "accessibility description",
        row(de_app, "accessibility").subtitle.as_deref().unwrap(),
        row(en_app, "accessibility").subtitle.as_deref().unwrap(),
    );
}
