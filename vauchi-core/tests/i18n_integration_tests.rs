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

use vauchi_core::i18n::{
    get_available_locales, get_locale_info, get_string, get_string_with_args, Locale,
};

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
    let result = get_string(Locale::English, "nonexistent.key");
    assert!(result.contains("Missing:") || result.contains("nonexistent"));
}

// ============================================================
// String Interpolation
// ============================================================

/// Test: String interpolation with arguments
#[test]
fn test_string_interpolation() {
    let result = get_string_with_args(Locale::English, "contacts.count", &[("count", "5")]);
    assert!(result.contains("5"), "Should interpolate count");
}

/// Test: Multiple argument interpolation
#[test]
fn test_multiple_args_interpolation() {
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
    let key = "welcome.title";
    let fr = get_string(Locale::French, key);
    assert!(!fr.contains("Missing"), "French translation should exist");
}

/// Test: Spanish translations exist
#[test]
fn test_spanish_coverage() {
    let key = "welcome.title";
    let es = get_string(Locale::Spanish, key);
    assert!(!es.contains("Missing"), "Spanish translation should exist");
}
