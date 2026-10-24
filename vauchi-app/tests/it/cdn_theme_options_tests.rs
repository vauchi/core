// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A theme published on the CDN is offered in Settings without an app
//! release (vauchi/private#731). The theme store is process-global; nextest
//! runs each test in its own process, so loading one here leaks nowhere.

use vauchi_app::ui::{AppEngine, AppScreen, Component};
use vauchi_core::api::Vauchi;

const CDN_THEMES: &[u8] = br##"[{
    "id": "cdn-only","name": "From the CDN","version": "1.0.0","mode": "dark",
    "colors": {
        "bg-primary": "#010203","bg-secondary": "#111111","bg-tertiary": "#222222",
        "text-primary": "#ffffff","text-secondary": "#cccccc",
        "accent": "#0000ff","accent-dark": "#000099",
        "success": "#00ff00","error": "#ff0000","warning": "#ffff00","border": "#333333"
    }
}]"##;

fn theme_options(engine: &mut AppEngine) -> Vec<(String, String)> {
    engine
        .navigate_to(AppScreen::SettingsAppearance)
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, options, .. } if id == "theme" => Some(
                options
                    .iter()
                    .map(|o| (o.id.clone(), o.label.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .expect("Settings has a theme dropdown")
}

// @internal
#[test]
fn a_theme_loaded_from_the_cdn_is_offered_in_settings() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.reload_design_assets(Some(CDN_THEMES), None).unwrap();

    assert!(
        theme_options(&mut engine).contains(&("cdn-only".to_string(), "From the CDN".to_string())),
        "the theme dropdown must offer the CDN-loaded theme: {:?}",
        theme_options(&mut engine)
    );
}
