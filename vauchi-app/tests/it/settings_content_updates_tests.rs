// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Advanced settings let a person choose when content updates run: On,
//! Wi-Fi only or Off (vauchi/private#733 D6), and whether they also run
//! while the app is in the background. The choices are Core's and
//! survive a restart.

use vauchi_app::ui::{
    AppEngine, AppScreen, Component, SettingsItemKind, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::settings::ContentUpdates;

fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn content_updates_dropdown(engine: &mut AppEngine) -> (Option<String>, Vec<String>) {
    engine
        .navigate_to(AppScreen::SettingsAdvanced)
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown {
                id,
                selected,
                options,
                ..
            } if id == "content_updates" => Some((
                selected.clone(),
                options.iter().map(|o| o.id.clone()).collect(),
            )),
            _ => None,
        })
        .expect("Advanced settings has a content-updates dropdown")
}

// @internal
#[test]
fn advanced_settings_offer_the_three_choices_with_on_selected() {
    let mut engine = engine();

    let (selected, options) = content_updates_dropdown(&mut engine);

    assert_eq!(selected.as_deref(), Some("on"));
    assert_eq!(options, ["on", "wifi_only", "off"]);
}

// @internal
#[test]
fn picking_a_choice_applies_and_persists_it() {
    let mut engine = engine();
    engine.navigate_to(AppScreen::SettingsAdvanced);

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "content_updates".into(),
        item_id: "wifi_only".into(),
    });

    assert_eq!(
        engine.vauchi().config().content_updates,
        ContentUpdates::WifiOnly
    );
    assert_eq!(
        engine
            .vauchi()
            .load_settings_flags()
            .unwrap()
            .content_updates,
        ContentUpdates::WifiOnly
    );
    assert_eq!(
        content_updates_dropdown(&mut engine).0.as_deref(),
        Some("wifi_only")
    );
}

// @internal
#[test]
fn an_unknown_choice_changes_nothing() {
    let mut engine = engine();
    engine.navigate_to(AppScreen::SettingsAdvanced);

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "content_updates".into(),
        item_id: "always".into(),
    });

    assert_eq!(engine.vauchi().config().content_updates, ContentUpdates::On);
}

fn background_updates_toggle(engine: &mut AppEngine) -> bool {
    engine
        .navigate_to(AppScreen::SettingsAdvanced)
        .components
        .iter()
        .find_map(|c| match c {
            Component::SettingsGroup { id, items, .. } if id == "content" => {
                items.iter().find_map(|item| match item.kind {
                    SettingsItemKind::Toggle { enabled }
                        if item.id == "content_updates_in_background" =>
                    {
                        Some(enabled)
                    }
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("Advanced settings has a background content-updates toggle")
}

/// Content updates while the app is in the background are the person's
/// choice too (owner, 2026-10-22), on by default.
// @internal
#[test]
fn advanced_settings_offer_background_updates_switched_on() {
    let mut engine = engine();

    assert!(background_updates_toggle(&mut engine));
}

// @internal
#[test]
fn switching_off_background_updates_applies_and_persists_it() {
    let mut engine = engine();
    engine.navigate_to(AppScreen::SettingsAdvanced);

    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "content".into(),
        item_id: "content_updates_in_background".into(),
    });

    assert!(!engine.vauchi().config().content_updates_in_background);
    assert!(
        !engine
            .vauchi()
            .load_settings_flags()
            .unwrap()
            .content_updates_in_background
    );
    assert!(!background_updates_toggle(&mut engine));
}

// @internal
#[test]
fn with_background_updates_off_only_an_app_in_the_foreground_updates() {
    let mut engine = engine();
    engine
        .vauchi_mut()
        .config_mut()
        .content_updates_in_background = false;
    assert!(!engine.content_updates_allowed(), "before the app is shown");

    engine.initial_commands().unwrap();
    assert!(engine.content_updates_allowed(), "in the foreground");

    let _ = engine.dispatch(vauchi_core::Event::AppBackgrounded);
    assert!(!engine.content_updates_allowed(), "in the background");

    engine
        .vauchi_mut()
        .config_mut()
        .content_updates_in_background = true;
    assert!(
        engine.content_updates_allowed(),
        "in the background, switched on"
    );
}
