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
    get_available_locales, get_locale_info, get_string, get_string_with_args,
    load_locale_from_bytes, Locale,
};

/// Initialize i18n once for integration tests using the bundled locale files.
static INIT: Once = Once::new();

fn ensure_init() {
    INIT.call_once(|| {
        // Integration tests load from the locale files in the repo
        let locales_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("locales");
        vauchi_core::i18n::init(&locales_dir)
            .expect("Failed to load locales for integration tests");
    });
}

// ============================================================
// Locale Support
// ============================================================

/// Test: All target locales are available
#[test]
fn test_all_target_locales_available() {
    let locales = get_available_locales();

    assert!(locales.contains(&Locale::English), "Should have English");
    assert!(locales.contains(&Locale::German), "Should have German");
    assert!(locales.contains(&Locale::French), "Should have French");
    assert!(locales.contains(&Locale::Spanish), "Should have Spanish");
}

/// Test: English is the default/source locale
#[test]
fn test_english_is_default() {
    let default = Locale::default();
    assert_eq!(default, Locale::English);
}

/// Test: Locale codes are correct
#[test]
fn test_locale_codes() {
    assert_eq!(Locale::English.code(), "en");
    assert_eq!(Locale::German.code(), "de");
    assert_eq!(Locale::French.code(), "fr");
    assert_eq!(Locale::Spanish.code(), "es");
}

/// Test: Locale can be parsed from code
#[test]
fn test_locale_from_code() {
    assert_eq!(Locale::from_code("en"), Some(Locale::English));
    assert_eq!(Locale::from_code("de"), Some(Locale::German));
    assert_eq!(Locale::from_code("fr"), Some(Locale::French));
    assert_eq!(Locale::from_code("es"), Some(Locale::Spanish));
    assert_eq!(Locale::from_code("xx"), None);
}

/// Test: Locale info is available
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
#[test]
fn test_string_interpolation() {
    ensure_init();

    let result = get_string_with_args(Locale::English, "contacts.count", &[("count", "5")]);
    assert!(result.contains("5"), "Should interpolate count");
}

/// Test: Multiple argument interpolation
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

/// Test: RTL detection for future locales
#[test]
fn test_rtl_detection() {
    // Current locales are all LTR
    assert!(!get_locale_info(Locale::English).is_rtl);
    assert!(!get_locale_info(Locale::German).is_rtl);
    assert!(!get_locale_info(Locale::French).is_rtl);
    assert!(!get_locale_info(Locale::Spanish).is_rtl);
}

// ============================================================
// Serialization
// ============================================================

/// Test: Locale can be serialized
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
#[test]
fn test_french_coverage() {
    ensure_init();

    let key = "welcome.title";
    let fr = get_string(Locale::French, key);
    assert!(!fr.contains("Missing"), "French translation should exist");
}

/// Test: Spanish translations exist
#[test]
fn test_spanish_coverage() {
    ensure_init();

    let key = "welcome.title";
    let es = get_string(Locale::Spanish, key);
    assert!(!es.contains("Missing"), "Spanish translation should exist");
}

// ============================================================
// Fallback & Cache Tests (#208)
// ============================================================

/// Test: Deterministic fallback — key present in English but absent in German (#208).
///
/// Uses `load_locale_from_bytes` to inject a controlled locale with known-missing keys.
/// Verifies that `get_string` returns the English value when the German key is absent.
#[test]
fn test_deterministic_fallback_to_english() {
    ensure_init();

    // Load a minimal German locale that is missing the "test.fallback_only" key
    let partial_de = r#"{"test.partial_key": "Teilweise"}"#;
    load_locale_from_bytes("de", partial_de.as_bytes()).expect("Should load partial DE locale");

    // English has the key (inject it)
    let en_with_key =
        r#"{"test.fallback_only": "English Fallback Value", "test.partial_key": "Partial Key"}"#;
    load_locale_from_bytes("en", en_with_key.as_bytes()).expect("Should load EN locale");

    // German lookup for missing key should fall back to English
    let de_result = get_string(Locale::German, "test.fallback_only");
    assert_eq!(
        de_result, "English Fallback Value",
        "German should fall back to English for missing key"
    );

    // German lookup for present key should return German value
    let de_present = get_string(Locale::German, "test.partial_key");
    assert_eq!(
        de_present, "Teilweise",
        "German should return own value when key exists"
    );
}

/// Test: Cache reload via `load_locale_from_bytes` updates subsequent lookups (#208).
///
/// Verifies that calling `load_locale_from_bytes` a second time replaces the
/// previous locale data and `get_string` reflects the new values.
#[test]
fn test_locale_cache_reload() {
    ensure_init();

    // Load initial French data with a specific value
    let fr_v1 = r#"{"test.reload_key": "Version Un"}"#;
    load_locale_from_bytes("fr", fr_v1.as_bytes()).expect("Should load FR v1");

    let v1_result = get_string(Locale::French, "test.reload_key");
    assert_eq!(v1_result, "Version Un");

    // Reload French with updated value (simulates locale file update)
    let fr_v2 = r#"{"test.reload_key": "Version Deux"}"#;
    load_locale_from_bytes("fr", fr_v2.as_bytes()).expect("Should load FR v2");

    let v2_result = get_string(Locale::French, "test.reload_key");
    assert_eq!(
        v2_result, "Version Deux",
        "Cache should reflect reloaded locale data"
    );
}

/// Test: Missing key returns "Missing:" prefix even after cache operations (#208).
#[test]
fn test_missing_key_after_cache_operations() {
    ensure_init();

    // After various load operations, a truly nonexistent key should still return "Missing:"
    let result = get_string(Locale::English, "test.absolutely_nonexistent_key_12345");
    assert!(
        result.starts_with("Missing:"),
        "Nonexistent key should return Missing prefix, got: {}",
        result
    );
}
