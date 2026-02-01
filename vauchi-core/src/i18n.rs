// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Internationalization (i18n) System
//!
//! Provides localized strings for the app UI.
//! Supports English (source), German, French, and Spanish.
//!
//! Strings are loaded from JSON locale files at compile time via `include_str!`.
//! The canonical locale files live in `vauchi-core/locales/*.json`.
//!
//! Feature file: features/internationalization.feature (pending)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Supported locales
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Locale {
    #[serde(rename = "en")]
    #[default]
    English,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "es")]
    Spanish,
}

impl Locale {
    /// Get the ISO 639-1 language code
    pub fn code(&self) -> &'static str {
        match self {
            Locale::English => "en",
            Locale::German => "de",
            Locale::French => "fr",
            Locale::Spanish => "es",
        }
    }

    /// Parse a locale from its code
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_lowercase().as_str() {
            "en" | "en-us" | "en-gb" => Some(Locale::English),
            "de" | "de-de" | "de-at" | "de-ch" => Some(Locale::German),
            "fr" | "fr-fr" | "fr-ca" => Some(Locale::French),
            "es" | "es-es" | "es-mx" => Some(Locale::Spanish),
            _ => None,
        }
    }
}

/// Information about a locale
#[derive(Debug, Clone)]
pub struct LocaleInfo {
    pub code: &'static str,
    pub name: &'static str,
    pub english_name: &'static str,
    pub is_rtl: bool,
}

/// Get information about a locale
pub fn get_locale_info(locale: Locale) -> LocaleInfo {
    match locale {
        Locale::English => LocaleInfo {
            code: "en",
            name: "English",
            english_name: "English",
            is_rtl: false,
        },
        Locale::German => LocaleInfo {
            code: "de",
            name: "Deutsch",
            english_name: "German",
            is_rtl: false,
        },
        Locale::French => LocaleInfo {
            code: "fr",
            name: "Français",
            english_name: "French",
            is_rtl: false,
        },
        Locale::Spanish => LocaleInfo {
            code: "es",
            name: "Español",
            english_name: "Spanish",
            is_rtl: false,
        },
    }
}

/// Get all available locales
pub fn get_available_locales() -> Vec<Locale> {
    vec![
        Locale::English,
        Locale::German,
        Locale::French,
        Locale::Spanish,
    ]
}

/// Get a localized string by key
pub fn get_string(locale: Locale, key: &str) -> String {
    let strings = get_strings_for_locale(locale);
    if let Some(value) = strings.get(key) {
        return value.clone();
    }

    // Fallback to English
    if locale != Locale::English {
        let en_strings = get_strings_for_locale(Locale::English);
        if let Some(value) = en_strings.get(key) {
            return value.clone();
        }
    }

    format!("Missing: {}", key)
}

/// Get a localized string with argument interpolation
pub fn get_string_with_args(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let mut result = get_string(locale, key);

    for (name, value) in args {
        result = result.replace(&format!("{{{}}}", name), value);
    }

    result
}

// ============================================================
// JSON-based string loading (compile-time embedded)
// ============================================================

static EN_STRINGS: OnceLock<HashMap<String, String>> = OnceLock::new();
static DE_STRINGS: OnceLock<HashMap<String, String>> = OnceLock::new();
static FR_STRINGS: OnceLock<HashMap<String, String>> = OnceLock::new();
static ES_STRINGS: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Parse a locale JSON file into a string map, filtering out the `_meta` key.
fn parse_locale_json(json: &str) -> HashMap<String, String> {
    let raw: HashMap<String, serde_json::Value> =
        serde_json::from_str(json).expect("locale JSON is valid");
    raw.into_iter()
        .filter(|(k, _)| k != "_meta")
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
        .collect()
}

fn get_strings_for_locale(locale: Locale) -> &'static HashMap<String, String> {
    match locale {
        Locale::English => {
            EN_STRINGS.get_or_init(|| parse_locale_json(include_str!("../locales/en.json")))
        }
        Locale::German => {
            DE_STRINGS.get_or_init(|| parse_locale_json(include_str!("../locales/de.json")))
        }
        Locale::French => {
            FR_STRINGS.get_or_init(|| parse_locale_json(include_str!("../locales/fr.json")))
        }
        Locale::Spanish => {
            ES_STRINGS.get_or_init(|| parse_locale_json(include_str!("../locales/es.json")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_default() {
        assert_eq!(Locale::default(), Locale::English);
    }

    #[test]
    fn test_locale_codes() {
        assert_eq!(Locale::English.code(), "en");
        assert_eq!(Locale::German.code(), "de");
    }

    #[test]
    fn test_locale_from_code() {
        assert_eq!(Locale::from_code("en"), Some(Locale::English));
        assert_eq!(Locale::from_code("EN"), Some(Locale::English));
        assert_eq!(Locale::from_code("en-US"), Some(Locale::English));
        assert_eq!(Locale::from_code("xx"), None);
    }

    #[test]
    fn test_get_string_english() {
        let s = get_string(Locale::English, "welcome.title");
        assert_eq!(s, "Welcome to Vauchi");
    }

    #[test]
    fn test_get_string_german() {
        let s = get_string(Locale::German, "welcome.title");
        assert_eq!(s, "Willkommen bei Vauchi");
    }

    #[test]
    fn test_get_string_fallback() {
        // If a key doesn't exist in German, it should fall back to English
        let en = get_string(Locale::English, "app.name");
        let de = get_string(Locale::German, "app.name");
        assert_eq!(en, de);
    }

    #[test]
    fn test_get_string_missing() {
        let s = get_string(Locale::English, "nonexistent");
        assert!(s.contains("Missing"));
    }

    #[test]
    fn test_interpolation() {
        let s = get_string_with_args(Locale::English, "contacts.count", &[("count", "5")]);
        assert_eq!(s, "5 contacts");
    }

    #[test]
    fn test_available_locales() {
        let locales = get_available_locales();
        assert_eq!(locales.len(), 4);
    }

    #[test]
    fn test_all_locales_have_same_keys() {
        let en = get_strings_for_locale(Locale::English);
        for locale in [Locale::German, Locale::French, Locale::Spanish] {
            let strings = get_strings_for_locale(locale);
            for key in en.keys() {
                assert!(
                    strings.contains_key(key),
                    "{:?} is missing key: {}",
                    locale,
                    key
                );
            }
        }
    }

    #[test]
    fn test_meta_key_excluded() {
        let en = get_strings_for_locale(Locale::English);
        assert!(!en.contains_key("_meta"));
    }
}
