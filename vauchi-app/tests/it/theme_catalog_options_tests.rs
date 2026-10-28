// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The published theme catalog, once downloaded, is offered in Settings
//! under each theme's own name. The theme store is process-global; nextest
//! runs each test in its own process, so loading the catalog leaks nowhere.

use vauchi_app::ui::{AppEngine, AppScreen, Component};
use vauchi_core::api::Vauchi;

fn published_catalog() -> Vec<u8> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/generated/themes.json");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn offered_theme_labels() -> Vec<String> {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine
        .reload_design_assets(Some(&published_catalog()), None)
        .unwrap();
    engine
        .navigate_to(AppScreen::SettingsAppearance)
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, options, .. } if id == "theme" => {
                Some(options.iter().map(|o| o.label.clone()).collect())
            }
            _ => None,
        })
        .expect("Settings has a theme dropdown")
}

fn assert_offered(expected: &[&str]) {
    let offered = offered_theme_labels();
    for label in expected {
        assert!(
            offered.iter().any(|o| o == label),
            "{label:?} is not offered: {offered:?}"
        );
    }
}

// @scenario: theming :: Catppuccin flavor themes available
#[test]
fn every_catppuccin_flavor_is_offered_under_its_catppuccin_name() {
    assert_offered(&[
        "Catppuccin Latte",
        "Catppuccin Frappé",
        "Catppuccin Macchiato",
        "Catppuccin Mocha",
    ]);
}

// @scenario: theming :: Dracula theme available
#[test]
fn dracula_is_offered() {
    assert_offered(&["Dracula"]);
}

// @scenario: theming :: Nord theme available
#[test]
fn nord_is_offered() {
    assert_offered(&["Nord"]);
}

// @scenario: theming :: Solarized themes available
#[test]
fn both_solarized_variants_are_offered() {
    assert_offered(&["Solarized Dark", "Solarized Light"]);
}

// @scenario: theming :: Gruvbox themes available
#[test]
fn both_gruvbox_variants_are_offered() {
    assert_offered(&["Gruvbox Dark", "Gruvbox Light"]);
}
