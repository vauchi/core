// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A language published on the CDN is offered in Settings without an app
//! release: once its file has loaded, under its own name, and as soon as
//! the manifest lists it, by its code (vauchi/private#733 D3). The string
//! store is process-global; nextest runs each test in its own process.

use vauchi_app::i18n::{load_locale_from_bytes, set_listed_locales};
use vauchi_app::ui::{AppEngine, AppScreen, Component};
use vauchi_core::api::Vauchi;

fn language_options() -> Vec<(String, String)> {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
        .navigate_to(AppScreen::SettingsAppearance)
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, options, .. } if id == "language" => Some(
                options
                    .iter()
                    .filter(|o| o.id != "follow_system")
                    .map(|o| (o.id.clone(), o.label.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .expect("Settings has a language dropdown")
}

fn offered_ids() -> Vec<String> {
    language_options().into_iter().map(|(id, _)| id).collect()
}

// @internal
#[test]
fn a_language_whose_file_loaded_is_offered_under_its_own_name() {
    load_locale_from_bytes(
        "pt",
        r#"{"_meta":{"locale":"pt","name":"Português","english_name":"Portuguese","is_rtl":false}}"#
            .as_bytes(),
    )
    .unwrap();

    assert!(
        language_options().contains(&("pt".to_string(), "Português".to_string())),
        "{:?}",
        language_options()
    );
}

// @internal
#[test]
fn a_listed_language_without_its_file_is_offered_by_its_code() {
    set_listed_locales(["en", "sw"]);

    assert!(
        language_options().contains(&("sw".to_string(), "sw".to_string())),
        "{:?}",
        language_options()
    );
}

// @internal
#[test]
fn a_listed_entry_that_is_not_a_language_code_is_not_offered() {
    set_listed_locales(["pt-BR", "engl", "../x", "", "E", "sw\0"]);

    let offered = offered_ids();

    assert_eq!(offered, vec!["en".to_string()], "{offered:?}");
}

// @internal
#[test]
fn english_is_offered_with_no_file_loaded_and_none_listed() {
    assert_eq!(
        language_options(),
        vec![("en".to_string(), "English".to_string())]
    );
}

// @internal
#[test]
fn a_language_is_offered_once_when_it_is_both_loaded_and_listed() {
    load_locale_from_bytes(
        "de",
        r#"{"_meta":{"locale":"de","name":"Deutsch","english_name":"German","is_rtl":false}}"#
            .as_bytes(),
    )
    .unwrap();
    set_listed_locales(["de", "en"]);

    let offered = offered_ids();

    assert_eq!(
        offered.iter().filter(|id| *id == "de").count(),
        1,
        "{offered:?}"
    );
    assert_eq!(
        offered.iter().filter(|id| *id == "en").count(),
        1,
        "{offered:?}"
    );
    assert_eq!(offered[0], "en", "English leads the list: {offered:?}");
}

/// No frontend mirrors its layout yet, so a right-to-left file from the CDN
/// loads but is not offered — the same guard the RTL canary keeps on the
/// bundled set (i18n_integration_tests).
// @internal
#[test]
fn a_right_to_left_language_is_not_offered_until_layouts_mirror() {
    load_locale_from_bytes(
        "fa",
        r#"{"_meta":{"locale":"fa","name":"فارسی","english_name":"Persian","is_rtl":true}}"#
            .as_bytes(),
    )
    .unwrap();
    set_listed_locales(["fa"]);

    assert!(
        !offered_ids().contains(&"fa".to_string()),
        "{:?}",
        offered_ids()
    );
}
