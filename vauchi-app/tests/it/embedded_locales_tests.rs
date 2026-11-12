// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Core embeds English, German, French, Spanish and Italian, so each
//! renders in its own words with no shell file and no content cycle; a
//! downloaded file still takes over a language (vauchi/private#1246).

use vauchi_app::i18n::{Locale, get_available_locales, get_string};

use super::i18n_support::locales_dir;

fn file_value(code: &str, key: &str) -> String {
    let bytes = std::fs::read(locales_dir().join(format!("{code}.json")))
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    let file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    file[key].as_str().expect("key in locale file").to_owned()
}

// @internal
#[test]
fn every_embedded_language_renders_its_own_words() {
    for code in ["de", "fr", "es", "it"] {
        let locale = Locale::from_code(code).unwrap();
        let expected = file_value(code, "onboarding.welcome_multidevice_title");

        let rendered = get_string(locale, "onboarding.welcome_multidevice_title");

        assert_eq!(rendered, expected, "locale {code}");
        assert_ne!(
            rendered,
            file_value("en", "onboarding.welcome_multidevice_title"),
            "locale {code} fell back to English"
        );
    }
}

// @internal
#[test]
fn settings_offers_every_embedded_language() {
    let offered: Vec<String> = get_available_locales()
        .iter()
        .map(|locale| locale.code().to_owned())
        .collect();

    for code in ["en", "de", "fr", "es", "it"] {
        assert!(
            offered.iter().any(|offered| offered == code),
            "{code} missing from {offered:?}"
        );
    }
}
