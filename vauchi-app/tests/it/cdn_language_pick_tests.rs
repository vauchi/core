// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Picking a language Core does not ship renders it at once when its file
//! has loaded, and once the file arrives later and the engine reloads its
//! locales, without a restart (vauchi/private#733 D4). The string store is
//! process-global; nextest runs each test in its own process.

use vauchi_app::i18n::load_locale_from_bytes;
use vauchi_app::ui::{AppEngine, AppScreen, RenderContext};
use vauchi_core::api::Vauchi;

const PORTUGUESE: &str = r#"{
    "_meta": {"locale": "pt", "name": "Português", "english_name": "Portuguese", "is_rtl": false},
    "contacts.title": "Pessoas"
}"#;

fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn picked(locale: &str, system: Option<&str>) -> RenderContext {
    RenderContext {
        locale: Some(locale.to_string()),
        system_locale: system.map(str::to_string),
        ..RenderContext::default()
    }
}

// @internal
#[test]
fn a_picked_cdn_language_whose_file_loaded_renders_at_once() {
    load_locale_from_bytes("pt", PORTUGUESE.as_bytes()).unwrap();
    let mut engine = engine();
    engine.navigate_to(AppScreen::Contacts);

    engine.set_render_context(picked("pt", None));

    assert_eq!(engine.navigate_to(AppScreen::Contacts).title, "Pessoas");
}

// @internal
#[test]
fn a_picked_language_renders_once_its_file_arrives_without_a_restart() {
    let mut engine = engine();
    engine.set_render_context(picked("pt", None));
    assert_eq!(engine.navigate_to(AppScreen::Contacts).title, "Contacts");

    load_locale_from_bytes("pt", PORTUGUESE.as_bytes()).unwrap();
    engine.reload_locales();

    assert_eq!(engine.navigate_to(AppScreen::Contacts).title, "Pessoas");
}
