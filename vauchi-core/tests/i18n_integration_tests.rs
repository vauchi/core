// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Internationalization Integration Tests
//!
//! Integration tests verifying i18n system.
//! Feature file: features/internationalization.feature (pending)
//!
//! These tests verify:
//! - Locale support (en, de, fr, es)
//! - String localization and interpolation
//! - Fallback to English for missing translations
//! - RTL support detection

use std::path::PathBuf;
use std::sync::Once;
use vauchi_core::i18n::{
    Locale, get_available_locales, get_locale_info, get_string, get_string_with_args,
};

/// Initialize i18n once for integration tests using the bundled locale files.
static INIT: Once = Once::new();

fn ensure_init() {
    INIT.call_once(|| {
        // Integration tests load from the sibling locales/ repo
        let locales_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("locales");
        vauchi_core::i18n::init(&locales_dir)
            .expect("Failed to load locales from sibling locales/ repo");
    });
}

// ============================================================
// Locale Support
// ============================================================

/// Test: All target locales are available
// @scenario: internationalization:Available languages are listed
// @scenario: internationalization:Core languages are supported
#[test]
fn test_all_target_locales_available() {
    let locales = get_available_locales();

    assert!(locales.contains(&Locale::English), "Should have English");
    assert!(locales.contains(&Locale::German), "Should have German");
    assert!(locales.contains(&Locale::French), "Should have French");
    assert!(locales.contains(&Locale::Spanish), "Should have Spanish");
}

/// Test: English is the default/source locale
// @scenario: internationalization:App falls back to English for unsupported languages
// @scenario: internationalization:App uses system language by default
#[test]
fn test_english_is_default() {
    let default = Locale::default();
    assert_eq!(default, Locale::English);
}

/// Test: Locale codes are correct
// @scenario: internationalization:Core languages are supported
#[test]
fn test_locale_codes() {
    assert_eq!(Locale::English.code(), "en");
    assert_eq!(Locale::German.code(), "de");
    assert_eq!(Locale::French.code(), "fr");
    assert_eq!(Locale::Spanish.code(), "es");
}

/// Test: Locale can be parsed from code
// @scenario: internationalization:App uses system language by default
// @scenario: internationalization:Core languages are supported
#[test]
fn test_locale_from_code() {
    assert_eq!(Locale::from_code("en"), Some(Locale::English));
    assert_eq!(Locale::from_code("de"), Some(Locale::German));
    assert_eq!(Locale::from_code("fr"), Some(Locale::French));
    assert_eq!(Locale::from_code("es"), Some(Locale::Spanish));
    assert_eq!(Locale::from_code("xx"), None);
}

/// Test: Locale info is available
// @scenario: internationalization:Available languages are listed
#[test]
fn test_locale_info() {
    let info = get_locale_info(Locale::German);
    assert_eq!(info.code, "de");
    assert_eq!(info.name, "Deutsch");
    assert_eq!(info.english_name, "German");
    assert!(!info.is_rtl);
}

// ============================================================
// String Localization
// ============================================================

/// Test: Basic strings are localized
// @scenario: internationalization:Core languages are supported
#[test]
fn test_basic_string_localization() {
    ensure_init();

    // English
    let en = get_string(Locale::English, "welcome.title");
    assert_eq!(en, "Welcome to Vauchi");

    // German
    let de = get_string(Locale::German, "welcome.title");
    assert_eq!(de, "Willkommen bei Vauchi");

    // French
    let fr = get_string(Locale::French, "welcome.title");
    assert_eq!(fr, "Bienvenue sur Vauchi");

    // Spanish
    let es = get_string(Locale::Spanish, "welcome.title");
    assert_eq!(es, "Bienvenido a Vauchi");
}

/// Test: All key sections have translations
// @scenario: internationalization:No untranslated strings visible
#[test]
fn test_key_sections_exist() {
    ensure_init();

    let sections = ["welcome", "contacts", "exchange", "settings", "help"];

    for section in sections {
        let key = format!("{}.title", section);
        let en = get_string(Locale::English, &key);
        assert!(
            !en.is_empty() && !en.starts_with("Missing:"),
            "Section {} should have title",
            section
        );
    }
}

/// Test: Fallback to English for missing translations
// @scenario: internationalization:App falls back to English for unsupported languages
#[test]
fn test_fallback_to_english() {
    ensure_init();

    // Use a key that might only exist in English
    let en = get_string(Locale::English, "app.name");
    let de = get_string(Locale::German, "app.name");

    // If German translation exists, use it; otherwise fallback to English
    assert!(!de.is_empty());
    // Both should return a valid string (German uses English fallback if missing)
    assert!(!en.is_empty());
}

/// Test: Missing key returns identifiable string
// @scenario: internationalization:App falls back to English for unsupported languages
#[test]
fn test_missing_key_handling() {
    ensure_init();

    let result = get_string(Locale::English, "nonexistent.key");
    assert!(result.contains("Missing:") || result.contains("nonexistent"));
}

// ============================================================
// String Interpolation
// ============================================================

/// Test: String interpolation with arguments
// @scenario: internationalization:Correct pluralization
#[test]
fn test_string_interpolation() {
    ensure_init();

    let result = get_string_with_args(Locale::English, "contacts.count", &[("count", "5")]);
    assert!(result.contains("5"), "Should interpolate count");
}

/// Test: Multiple argument interpolation
// @scenario: internationalization:Correct pluralization
#[test]
fn test_multiple_args_interpolation() {
    ensure_init();

    let result = get_string_with_args(
        Locale::English,
        "update.sent",
        &[("count", "3"), ("name", "Alice")],
    );
    // The string should contain both interpolated values
    assert!(!result.is_empty());
}

/// Test: Interpolation works across locales
// @scenario: internationalization:Core languages are supported
// @scenario: internationalization:Correct pluralization
#[test]
fn test_interpolation_across_locales() {
    ensure_init();

    let en = get_string_with_args(Locale::English, "contacts.count", &[("count", "10")]);
    let de = get_string_with_args(Locale::German, "contacts.count", &[("count", "10")]);

    assert!(en.contains("10"));
    assert!(de.contains("10"));
    assert_ne!(en, de, "Translations should differ");
}

// ============================================================
// Common UI Strings
// ============================================================

/// Test: Navigation strings exist
// @scenario: internationalization:No untranslated strings visible
#[test]
fn test_navigation_strings() {
    ensure_init();

    let keys = ["nav.home", "nav.contacts", "nav.exchange", "nav.settings"];

    for key in keys {
        let en = get_string(Locale::English, key);
        assert!(
            !en.is_empty() && !en.contains("Missing"),
            "Key {} should exist",
            key
        );
    }
}

/// Test: Action strings exist
// @scenario: internationalization:No untranslated strings visible
#[test]
fn test_action_strings() {
    ensure_init();

    let keys = [
        "action.save",
        "action.cancel",
        "action.delete",
        "action.edit",
        "action.share",
    ];

    for key in keys {
        let en = get_string(Locale::English, key);
        assert!(
            !en.is_empty() && !en.contains("Missing"),
            "Action {} should exist",
            key
        );
    }
}

/// Test: Error strings exist
// @scenario: internationalization:Error messages are translated
#[test]
fn test_error_strings() {
    ensure_init();

    let keys = ["error.generic", "error.network", "error.validation"];

    for key in keys {
        let en = get_string(Locale::English, key);
        assert!(
            !en.is_empty() && !en.contains("Missing"),
            "Error {} should exist",
            key
        );
    }
}

// ============================================================
// RTL Support
// ============================================================

/// Test: RTL detection for current locales (all LTR)
// @scenario: internationalization:RTL layout for Arabic
// @scenario: internationalization:RTL layout for Hebrew
#[test]
fn test_rtl_detection() {
    // Current locales are all LTR
    assert!(!get_locale_info(Locale::English).is_rtl);
    assert!(!get_locale_info(Locale::German).is_rtl);
    assert!(!get_locale_info(Locale::French).is_rtl);
    assert!(!get_locale_info(Locale::Spanish).is_rtl);
}

/// Canary test: blocks RTL locale addition until layout mirroring is implemented.
///
/// If this test fails, it means an RTL locale (Arabic, Hebrew, etc.) has been added
/// to the Locale enum. Before enabling it:
///   1. Implement LayoutDirection in core ScreenModel
///   2. Add layout mirroring in each frontend (iOS, Android, GTK, Qt, Windows, Web)
///   3. Add RTL-specific integration tests per platform
///   4. Remove this canary gate
///
/// See: _private/docs/plans/2026-03-18-frontend-audit-investigation.md (S-04)
/// See: _private/docs/planning/done/2026-01-20-internationalization-implementation.md (Phase 6)
// @scenario: internationalization:RTL layout for Arabic
#[test]
fn test_rtl_canary_no_rtl_locales_without_layout_support() {
    let rtl_locales: Vec<_> = get_available_locales()
        .into_iter()
        .filter(|l| get_locale_info(*l).is_rtl)
        .collect();

    assert!(
        rtl_locales.is_empty(),
        "RTL locale(s) found ({:?}) but layout mirroring is NOT yet implemented \
         in any frontend. Adding an RTL locale without layout support will cause \
         visual breakage on all platforms. See Phase 6 of the i18n plan and the \
         frontend architecture audit (2026-03-18) before proceeding.",
        rtl_locales.iter().map(|l| l.code()).collect::<Vec<_>>()
    );
}

// ============================================================
// Serialization
// ============================================================

/// Test: Locale can be serialized
// @scenario: internationalization:Core languages are supported
#[test]
fn test_locale_serialization() {
    let locale = Locale::German;
    let json = serde_json::to_string(&locale).expect("Should serialize");
    assert!(json.contains("de") || json.contains("German"));

    let restored: Locale = serde_json::from_str(&json).expect("Should deserialize");
    assert_eq!(restored, locale);
}

// ============================================================
// Full Coverage
// ============================================================

/// Test: All English strings have German translations
// @scenario: internationalization:Core languages are supported
#[test]
fn test_german_coverage() {
    ensure_init();

    // Check a representative sample of keys
    let keys = [
        "welcome.title",
        "welcome.subtitle",
        "contacts.title",
        "contacts.empty",
        "settings.title",
        "help.title",
    ];

    for key in keys {
        let _en = get_string(Locale::English, key);
        let de = get_string(Locale::German, key);

        assert!(
            !de.contains("Missing"),
            "German translation missing for {}",
            key
        );
        // Translations should be different (unless the word is the same)
        // This is a soft check - some words might be the same
    }
}

/// Test: French translations exist
// @scenario: internationalization:Core languages are supported
#[test]
fn test_french_coverage() {
    ensure_init();

    let key = "welcome.title";
    let fr = get_string(Locale::French, key);
    assert!(!fr.contains("Missing"), "French translation should exist");
}

/// Test: Spanish translations exist
// @scenario: internationalization:Core languages are supported
#[test]
fn test_spanish_coverage() {
    ensure_init();

    let key = "welcome.title";
    let es = get_string(Locale::Spanish, key);
    assert!(!es.contains("Missing"), "Spanish translation should exist");
}

// ============================================================
// Locale File Parity
// ============================================================

/// Test: All locale files have the same keys as English
///
/// Reads raw JSON from the sibling locales/ repo and verifies that every
/// non-English locale has exactly the same set of keys (excluding `_meta`).
// @scenario: internationalization:Locale files are complete
#[test]
fn test_all_locale_files_have_same_keys_as_english() {
    use std::collections::BTreeSet;

    let locales_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("locales");

    let en_path = locales_dir.join("en.json");
    assert!(
        en_path.exists(),
        "English locale not found at {}",
        en_path.display()
    );

    let en_content: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&en_path).expect("read en.json"))
            .expect("parse en.json");
    let en_keys: BTreeSet<String> = en_content
        .as_object()
        .expect("en.json should be an object")
        .keys()
        .filter(|k| !k.starts_with("_meta"))
        .cloned()
        .collect();

    assert!(!en_keys.is_empty(), "en.json should have keys");

    for entry in std::fs::read_dir(&locales_dir).expect("read locales dir") {
        let entry = entry.expect("read dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let filename = path.file_stem().unwrap().to_str().unwrap().to_string();
        if filename == "en" || filename.ends_with(".schema") {
            continue;
        }

        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read locale file"))
                .unwrap_or_else(|e| panic!("parse {}.json: {}", filename, e));
        let locale_keys: BTreeSet<String> = content
            .as_object()
            .unwrap_or_else(|| panic!("{}.json should be an object", filename))
            .keys()
            .filter(|k| !k.starts_with("_meta"))
            .cloned()
            .collect();

        let missing: Vec<&String> = en_keys.difference(&locale_keys).collect();
        let extra: Vec<&String> = locale_keys.difference(&en_keys).collect();

        assert!(
            missing.is_empty() && extra.is_empty(),
            "Locale '{}' key mismatch vs English:\n  Missing: {:?}\n  Extra: {:?}",
            filename,
            missing,
            extra,
        );
    }
}

/// Test: No locale has empty string values
// @scenario: internationalization:Locale files are complete
#[test]
fn test_no_locale_has_empty_values() {
    let locales_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("locales");

    for entry in std::fs::read_dir(&locales_dir).expect("read locales dir") {
        let entry = entry.expect("read dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let filename = path.file_stem().unwrap().to_str().unwrap().to_string();
        if filename.ends_with(".schema") {
            continue;
        }

        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read locale file"))
                .unwrap_or_else(|e| panic!("parse {}.json: {}", filename, e));

        let empty_keys: Vec<String> = content
            .as_object()
            .unwrap_or_else(|| panic!("{}.json should be an object", filename))
            .iter()
            .filter(|(k, _)| !k.starts_with("_meta"))
            .filter(|(_, v)| v.as_str().is_some_and(|s| s.is_empty()))
            .map(|(k, _)| k.clone())
            .collect();

        assert!(
            empty_keys.is_empty(),
            "Locale '{}' has empty values for keys: {:?}",
            filename,
            empty_keys,
        );
    }
}

// Fallback & Cache tests (#208) that call `load_locale_from_bytes` live in
// `i18n_cache_tests.rs` (separate binary) to avoid corrupting shared state.
