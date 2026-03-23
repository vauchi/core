// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! i18n Cache & Reload Tests (#208)
//!
//! These tests call `load_locale_from_bytes` which replaces global locale data.
//! They live in a separate binary from `i18n_integration_tests` so they don't
//! corrupt shared state for tests that read real locale keys in parallel.

use std::path::PathBuf;
use std::sync::Once;
use vauchi_core::i18n::{Locale, get_string, load_locale_from_bytes};

static INIT: Once = Once::new();

fn ensure_init() {
    INIT.call_once(|| {
        let locales_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        vauchi_core::i18n::init(&locales_dir)
            .expect("Failed to load locales from sibling locales/ repo");
    });
}

/// Test: Deterministic fallback — key present in English but absent in German (#208).
// @scenario: internationalization :: App falls back to English for unsupported languages
#[test]
fn test_deterministic_fallback_to_english() {
    ensure_init();

    let partial_de = r#"{"test.partial_key": "Teilweise"}"#;
    load_locale_from_bytes("de", partial_de.as_bytes()).expect("Should load partial DE locale");

    let en_with_key =
        r#"{"test.fallback_only": "English Fallback Value", "test.partial_key": "Partial Key"}"#;
    load_locale_from_bytes("en", en_with_key.as_bytes()).expect("Should load EN locale");

    let de_result = get_string(Locale::German, "test.fallback_only");
    assert_eq!(
        de_result, "English Fallback Value",
        "German should fall back to English for missing key"
    );

    let de_present = get_string(Locale::German, "test.partial_key");
    assert_eq!(
        de_present, "Teilweise",
        "German should return own value when key exists"
    );
}

/// Test: Cache reload via `load_locale_from_bytes` updates subsequent lookups (#208).
// @scenario: internationalization :: Language change applies immediately
#[test]
fn test_locale_cache_reload() {
    ensure_init();

    let fr_v1 = r#"{"test.reload_key": "Version Un"}"#;
    load_locale_from_bytes("fr", fr_v1.as_bytes()).expect("Should load FR v1");

    let v1_result = get_string(Locale::French, "test.reload_key");
    assert_eq!(v1_result, "Version Un");

    let fr_v2 = r#"{"test.reload_key": "Version Deux"}"#;
    load_locale_from_bytes("fr", fr_v2.as_bytes()).expect("Should load FR v2");

    let v2_result = get_string(Locale::French, "test.reload_key");
    assert_eq!(
        v2_result, "Version Deux",
        "Cache should reflect reloaded locale data"
    );
}

/// Test: Missing key returns "Missing:" prefix even after cache operations (#208).
// @scenario: internationalization :: App falls back to English for unsupported languages
#[test]
fn test_missing_key_after_cache_operations() {
    ensure_init();

    let result = get_string(Locale::English, "test.absolutely_nonexistent_key_12345");
    assert!(
        result.starts_with("Missing:"),
        "Nonexistent key should return Missing prefix, got: {}",
        result
    );
}
