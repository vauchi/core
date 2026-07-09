// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Internationalization (i18n) System
//!
//! Provides localized strings for the app UI.
//! Supports English (source), German, French, and Spanish.
//!
//! Strings are loaded at runtime from JSON locale files via `init()`.
//! If no locale files are loaded, a minimal hardcoded English fallback is used.
//!
//! Feature file: features/internationalization.feature (pending)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use thiserror::Error;

/// Supported locales
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
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
    #[serde(rename = "it")]
    Italian,
}

impl Locale {
    /// Get the ISO 639-1 language code
    pub fn code(&self) -> &'static str {
        match self {
            Locale::English => "en",
            Locale::German => "de",
            Locale::French => "fr",
            Locale::Spanish => "es",
            Locale::Italian => "it",
        }
    }

    /// Parse a locale from its code
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_lowercase().as_str() {
            "en" | "en-us" | "en-gb" => Some(Locale::English),
            "de" | "de-de" | "de-at" | "de-ch" => Some(Locale::German),
            "fr" | "fr-fr" | "fr-ca" => Some(Locale::French),
            "es" | "es-es" | "es-mx" => Some(Locale::Spanish),
            "it" | "it-it" | "it-ch" => Some(Locale::Italian),
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
        Locale::Italian => LocaleInfo {
            code: "it",
            name: "Italiano",
            english_name: "Italian",
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
        Locale::Italian,
    ]
}

/// Get a localized string by key
pub fn get_string(locale: Locale, key: &str) -> String {
    if let Some(value) = lookup_one(locale, key) {
        return value;
    }

    // Fallback to English
    if locale != Locale::English
        && let Some(value) = lookup_one(Locale::English, key)
    {
        return value;
    }

    format!("Missing: {}", key)
}

/// Single-key lookup — clones only the matched value, never a whole
/// locale map. A store-loaded locale wins entirely (hit or miss),
/// matching [`get_strings_for_locale`]'s fallback order; only an
/// UNLOADED English falls back to the cached bundled set. Screen
/// builders call this dozens of times per render, so the per-call
/// full-map clone / full-JSON parse this replaces made engine
/// proptests time out (M3 S4a, 2026-07-04).
fn lookup_one(locale: Locale, key: &str) -> Option<String> {
    if let Ok(lock) = LOCALE_STORE.read()
        && let Some(store) = lock.as_ref()
        && let Some(strings) = store.get(locale.code())
    {
        return strings.get(key).cloned();
    }
    if locale == Locale::English {
        return bundled_english_cached().get(key).cloned();
    }
    None
}

/// Get a localized string with argument interpolation
/// Get all localized strings for a locale as a key-value map.
/// Returns English strings with overrides from the specified locale.
pub fn get_all_strings(locale: Locale) -> HashMap<String, String> {
    get_strings_for_locale(locale)
}

/// Returns a localized string with named placeholders replaced by the provided arguments.
pub fn get_string_with_args(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let mut result = get_string(locale, key);

    for (name, value) in args {
        result = result.replace(&format!("{{{}}}", name), value);
    }

    result
}

// ============================================================
// Runtime locale store (RwLock-based, supports reload)
// ============================================================

/// Global locale store: maps locale code → string map
static LOCALE_STORE: RwLock<Option<HashMap<String, HashMap<String, String>>>> = RwLock::new(None);

/// Errors from i18n operations
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum I18nError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Lock poisoned")]
    LockPoisoned,

    #[error("Invalid filename: {0}")]
    InvalidFilename(String),
}

/// Initialize i18n by loading locale JSON files from a directory.
///
/// Scans `resource_dir` for `*.json` files, parses each into locale strings,
/// and stores them in the global locale store. The filename stem is used as
/// the locale code (e.g., `en.json` → `"en"`).
pub fn init(resource_dir: &Path) -> Result<(), I18nError> {
    let mut store = HashMap::new();

    if resource_dir.exists() {
        for entry in std::fs::read_dir(resource_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let code = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| I18nError::InvalidFilename(path.display().to_string()))?
                    .to_string();

                let data = std::fs::read(&path)?;
                let strings = parse_locale_bytes(&data)?;
                store.insert(code, strings);
            }
        }
    }

    let mut lock = LOCALE_STORE.write().map_err(|_| I18nError::LockPoisoned)?;
    *lock = Some(store);
    Ok(())
}

/// Load or reload a single locale from raw JSON bytes.
///
/// Called by the content update system after downloading locale files from CDN.
/// If the store hasn't been initialized yet, creates it first.
///
/// Merges the downloaded strings on top of the existing store entry (or the
/// bundled English fallback for the "en" locale) instead of replacing it
/// outright. This prevents a partial CDN locale file from wiping keys the
/// running app still needs, which would surface as "Missing: ..." placeholders.
pub fn load_locale_from_bytes(code: &str, data: &[u8]) -> Result<(), I18nError> {
    let mut strings = parse_locale_bytes(data)?;

    let mut lock = LOCALE_STORE.write().map_err(|_| I18nError::LockPoisoned)?;

    let store = lock.get_or_insert_with(HashMap::new);

    // Preserve keys the downloaded file is missing by backfilling from the
    // previous store entry, then from the bundled English fallback for en.
    if let Some(existing) = store.get(code) {
        for (key, value) in existing {
            strings.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    if code == "en" {
        for (key, value) in bundled_english_cached() {
            strings.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }

    store.insert(code.to_string(), strings);
    Ok(())
}

/// Check if any locales have been loaded into the store.
pub fn is_initialized() -> bool {
    LOCALE_STORE
        .read()
        .map(|lock| lock.as_ref().is_some_and(|s| !s.is_empty()))
        .unwrap_or(false)
}

/// Parse a locale JSON byte slice into a string map, filtering out the `_meta` key.
fn parse_locale_bytes(data: &[u8]) -> Result<HashMap<String, String>, I18nError> {
    let raw: HashMap<String, serde_json::Value> = serde_json::from_slice(data)?;
    Ok(raw
        .into_iter()
        .filter(|(k, _)| k != "_meta")
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
        .collect())
}

/// Get strings for a specific locale. Reads from the RwLock store, falling
/// back to `bundled_english()` if the store is empty or the locale is missing.
fn get_strings_for_locale(locale: Locale) -> HashMap<String, String> {
    let code = locale.code();

    if let Ok(lock) = LOCALE_STORE.read()
        && let Some(store) = lock.as_ref()
        && let Some(strings) = store.get(code)
    {
        return strings.clone();
    }

    // Fallback: if requesting English and nothing loaded, use bundled minimal set
    if locale == Locale::English {
        return bundled_english();
    }

    // For non-English locales with no data, return empty (caller will fall back to English)
    HashMap::new()
}

// Include the bundled locale generated by build.rs
include!(concat!(env!("OUT_DIR"), "/bundled_locale.rs"));

/// Bundled English strings — parsed from embedded locale JSON at compile time.
/// The JSON is sourced from the sibling `locales/` repo via build.rs.
fn bundled_english() -> HashMap<String, String> {
    let raw: HashMap<String, serde_json::Value> =
        serde_json::from_str(BUNDLED_EN_JSON).unwrap_or_default();

    raw.into_iter()
        .filter(|(k, _)| k != "_meta")
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
        .collect()
}

/// The bundled set parses once — every fallback lookup after that is a
/// map read. (Its predecessor's "cached after first call" comment was
/// aspirational: each call re-parsed the full JSON.)
fn bundled_english_cached() -> &'static HashMap<String, String> {
    static BUNDLED: std::sync::OnceLock<HashMap<String, String>> = std::sync::OnceLock::new();
    BUNDLED.get_or_init(bundled_english)
}

/// Shared test lock: any test that mutates `LOCALE_STORE` (clear, reset, init)
/// must hold this lock. Lives at module level so other test modules (help,
/// aha_moments) can acquire it too, preventing inter-module races.
#[cfg(test)]
pub(crate) static I18N_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// INLINE_TEST_REQUIRED: tests access private LOCALE_STORE global state and internal init/reset
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Alias for convenience — delegates to module-level lock.
    fn lock_store() -> std::sync::MutexGuard<'static, ()> {
        // Recover from poison: the mutex guards `()` so there's no state to corrupt.
        // This prevents cascade failures when a test panics while holding the lock.
        super::I18N_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// RAII guard that resets the global locale store to the real locale
    /// files when dropped. Use in any test that mutates the store with
    /// temporary/minimal locale data so later tests still see the full
    /// English strings.
    struct StoreGuard {
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for StoreGuard {
        fn drop(&mut self) {
            reset_store();
        }
    }
    fn store_guard() -> StoreGuard {
        StoreGuard {
            _guard: lock_store(),
        }
    }

    /// Helper: create a temp dir with locale JSON files for testing
    fn setup_test_locales() -> TempDir {
        let dir = TempDir::new().unwrap();
        let en = serde_json::json!({
            "_meta": { "locale": "en" },
            "app.name": "Vauchi",
            "welcome.title": "Welcome to Vauchi",
            "contacts.count": "{count} contacts",
            "contacts.title": "Contacts",
            "contacts.empty": "No contacts yet",
            "update.sent": "Sent {count} updates to {name}"
        });
        let de = serde_json::json!({
            "_meta": { "locale": "de" },
            "app.name": "Vauchi",
            "welcome.title": "Willkommen bei Vauchi",
            "contacts.count": "{count} Kontakte",
            "contacts.title": "Kontakte",
            "contacts.empty": "Noch keine Kontakte"
        });
        fs::write(dir.path().join("en.json"), en.to_string()).unwrap();
        fs::write(dir.path().join("de.json"), de.to_string()).unwrap();
        dir
    }

    /// Helper: reset the global store between tests by reloading from locale files.
    /// Uses the real locale directory instead of clearing to None, so other test modules
    /// (help, aha_moments) that rely on i18n data aren't broken by the reset.
    fn reset_store() {
        let locales_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        let _ = init(&locales_dir);
    }

    /// Helper: fully clear the store (only for tests that specifically test uninitialized state)
    fn clear_store() {
        let mut lock = LOCALE_STORE.write().unwrap_or_else(|e| e.into_inner());
        *lock = None;
    }

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
    fn test_init_loads_locales() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        assert!(is_initialized());
        let s = get_string(Locale::English, "welcome.title");
        assert_eq!(s, "Welcome to Vauchi");
    }

    #[test]
    fn test_init_german_strings() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let s = get_string(Locale::German, "welcome.title");
        assert_eq!(s, "Willkommen bei Vauchi");
    }

    #[test]
    fn test_fallback_to_english() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        // "update.sent" only exists in en, not de
        let de = get_string(Locale::German, "update.sent");
        let en = get_string(Locale::English, "update.sent");
        assert_eq!(de, en);
    }

    #[test]
    fn test_get_string_missing() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let s = get_string(Locale::English, "nonexistent");
        assert!(s.contains("Missing"));
    }

    #[test]
    fn test_interpolation() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let s = get_string_with_args(Locale::English, "contacts.count", &[("count", "5")]);
        assert_eq!(s, "5 contacts");
    }

    #[test]
    fn test_available_locales() {
        let locales = get_available_locales();
        assert_eq!(locales.len(), 5);
    }

    #[test]
    fn test_meta_key_excluded() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let strings = get_strings_for_locale(Locale::English);
        assert!(!strings.contains_key("_meta"));
    }

    #[test]
    fn test_reload_locale_updates_strings() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        assert_eq!(
            get_string(Locale::English, "welcome.title"),
            "Welcome to Vauchi"
        );

        // Reload with updated strings
        let updated = serde_json::json!({
            "welcome.title": "Welcome Back to Vauchi"
        });
        load_locale_from_bytes("en", updated.to_string().as_bytes()).unwrap();

        assert_eq!(
            get_string(Locale::English, "welcome.title"),
            "Welcome Back to Vauchi"
        );
    }

    #[test]
    fn test_bundled_english_fallback() {
        let _guard = store_guard();
        reset_store();
        // Without init(), should fall back to bundled_english
        let s = get_string(Locale::English, "app.name");
        assert_eq!(s, "Vauchi");
    }

    #[test]
    fn test_bundled_english_has_critical_keys() {
        let bundled = bundled_english();
        let critical = [
            "app.name",
            "nav.home",
            "nav.contacts",
            "nav.settings",
            "nav.groups",
            "nav.more",
            "nav.myCard",
            "error.generic",
            "action.save",
            "action.cancel",
        ];
        for key in critical {
            assert!(
                bundled.contains_key(key),
                "bundled_english missing: {}",
                key
            );
        }
    }

    #[test]
    fn test_concurrent_read_during_reload() {
        // allow(zero_assertions): Concurrency stress test — validates no panic under contention
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        // Spawn readers while we reload
        let handles: Vec<_> = (0..10)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..100 {
                        let _ = get_string(Locale::English, "welcome.title");
                    }
                })
            })
            .collect();

        // Reload in the middle
        let updated = serde_json::json!({ "welcome.title": "Updated" });
        load_locale_from_bytes("en", updated.to_string().as_bytes()).unwrap();

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_init_with_nonexistent_dir() {
        let _guard = store_guard();
        reset_store();
        let result = init(Path::new("/tmp/nonexistent-vauchi-i18n-test"));
        // Should succeed with empty store (dir doesn't exist = no files)
        result.expect("expected success");
    }

    #[test]
    fn test_is_initialized_false_before_init() {
        let _guard = store_guard();
        clear_store();
        assert!(!is_initialized());
        // Restore for other tests
        reset_store();
    }

    #[test]
    fn test_load_locale_from_bytes_without_init() {
        let _guard = store_guard();
        clear_store();
        let data = serde_json::json!({ "app.name": "Test" });
        load_locale_from_bytes("en", data.to_string().as_bytes()).unwrap();
        assert!(is_initialized());
        assert_eq!(get_string(Locale::English, "app.name"), "Test");
        // Restore for other tests
        reset_store();
    }

    #[test]
    fn test_all_strings_returns_full_map() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let all = get_all_strings(Locale::English);
        assert!(all.contains_key("welcome.title"));
        assert!(all.contains_key("contacts.count"));
        assert!(!all.contains_key("_meta"));
    }
}
