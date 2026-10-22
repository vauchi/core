// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! On "Follow system" the shells push no `locale`, so Core rendered
//! English whatever the OS language (vauchi/private#721). Shells now report
//! the OS tag as `system_locale`; Core resolves the explicit choice first,
//! then the system tag, then English, and a regional tag falls back to its
//! language.

use proptest::prelude::*;
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{AppEngine, AppScreen, Component, RenderContext, ScreenModel};
use vauchi_core::api::Vauchi;

use super::i18n_support::{assert_translated, load_german};

fn context(locale: Option<&str>, system_locale: Option<&str>) -> RenderContext {
    RenderContext {
        locale: locale.map(str::to_string),
        system_locale: system_locale.map(str::to_string),
        ..RenderContext::default()
    }
}

// @internal
#[test]
fn a_regional_tag_falls_back_to_its_language() {
    for (tag, expected) in [
        ("fr-CH", Locale::FRENCH),
        ("FR-ch", Locale::FRENCH),
        ("fr-BE", Locale::FRENCH),
        ("de_LI.UTF-8", Locale::GERMAN),
        ("it-CH", Locale::ITALIAN),
        ("es-AR", Locale::SPANISH),
        ("en-AU", Locale::ENGLISH),
    ] {
        assert_eq!(Locale::from_code(tag), Some(expected), "tag {tag:?}");
    }
}

// @internal
#[test]
fn a_tag_without_a_language_subtag_resolves_to_none() {
    let long = "a".repeat(1024);
    for tag in ["", "-", "_", ".", "C", "POSIX", "en\0de", long.as_str()] {
        assert_eq!(Locale::from_code(tag), None, "tag {tag:?}");
    }
}

// @internal
#[test]
fn the_system_language_is_used_when_no_language_was_picked() {
    assert_eq!(
        context(None, Some("fr-CH")).resolved_locale(),
        Locale::FRENCH
    );
}

// @internal
#[test]
fn a_picked_language_wins_over_the_system_language() {
    assert_eq!(
        context(Some("de"), Some("fr")).resolved_locale(),
        Locale::GERMAN
    );
}

// @internal
#[test]
fn an_unknown_picked_language_falls_through_to_the_system_language() {
    assert_eq!(
        context(Some("xx"), Some("fr")).resolved_locale(),
        Locale::FRENCH
    );
}

// @internal
#[test]
fn an_untranslated_system_language_resolves_to_english() {
    assert_eq!(
        context(None, Some("pt-BR")).resolved_locale(),
        Locale::ENGLISH
    );
    assert_eq!(context(None, None).resolved_locale(), Locale::ENGLISH);
}

proptest! {
    // @internal
    #[test]
    fn a_picked_language_that_parses_always_wins(
        picked in prop::sample::select(vec!["en", "de", "fr", "es", "it", "fr-CH", "de_AT"]),
        system in ".{0,40}",
    ) {
        prop_assert_eq!(
            context(Some(picked), Some(&system)).resolved_locale(),
            Locale::from_code(picked).unwrap()
        );
    }
}

fn help_headers(screen: &ScreenModel) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SectionedActionList { id, sections } if id == "help" => Some(
                sections
                    .iter()
                    .map(|s| s.label.as_str())
                    .collect::<Vec<_>>()
                    .join(" | "),
            ),
            _ => None,
        })
        .expect("help emits one `help` sectioned list")
}

fn language_selected(engine: &mut AppEngine) -> Option<String> {
    engine
        .navigate_to(AppScreen::SettingsAppearance)
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, selected, .. } if id == "language" => Some(selected.clone()),
            _ => None,
        })
        .expect("Settings has a language dropdown")
}

// @internal
#[test]
fn follow_system_renders_the_system_language_and_keeps_follow_system_selected() {
    load_german();
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let english = help_headers(&engine.navigate_to(AppScreen::Help));

    engine.set_render_context(context(None, Some("de-LI")));

    let system = help_headers(&engine.navigate_to(AppScreen::Help));
    assert_translated("help section headers", &system, &english);
    assert_eq!(
        language_selected(&mut engine).as_deref(),
        Some("follow_system")
    );
}

// @internal
#[test]
fn follow_system_renders_settings_in_the_system_language() {
    load_german();
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let english = [AppScreen::Settings, AppScreen::SettingsAppearance]
        .map(|screen| engine.navigate_to(screen).title);

    engine.set_render_context(context(None, Some("de-LI")));

    for (screen, english) in [AppScreen::Settings, AppScreen::SettingsAppearance]
        .into_iter()
        .zip(english)
    {
        let system = engine.navigate_to(screen.clone()).title;
        assert_translated(&format!("{screen:?} title"), &system, &english);
    }
}

// @internal
#[test]
fn a_screen_built_before_the_system_language_arrives_renders_in_it() {
    load_german();
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let english = engine.navigate_to(AppScreen::Contacts).title;
    engine.navigate_to(AppScreen::Help);

    engine.set_render_context(context(None, Some("de")));

    let system = engine.navigate_to(AppScreen::Contacts).title;
    assert_translated("Contacts title", &system, &english);
}
