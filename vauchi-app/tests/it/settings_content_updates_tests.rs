// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Advanced settings let a person choose when content updates run: On,
//! Wi-Fi only or Off (vauchi/private#733 D6). The choice is Core's and
//! survives a restart.

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
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
