// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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

/// A language, by its lower-case ISO 639 language subtag. Any
/// well-formed subtag is a `Locale`, so a language published on the CDN
/// needs no Core release (vauchi/private#733 D1); the constants name the
/// languages Core ships with.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Locale {
    subtag: [u8; 3],
    len: u8,
}

/// The longest tag `from_code` looks at; shells pass OS tags such as
/// `de_LI.UTF-8`, never anything near this (DC-01).
const MAX_TAG_BYTES: usize = 64;

impl Locale {
    pub const ENGLISH: Locale = Locale::two_letter(*b"en");
    pub const GERMAN: Locale = Locale::two_letter(*b"de");
    pub const FRENCH: Locale = Locale::two_letter(*b"fr");
    pub const SPANISH: Locale = Locale::two_letter(*b"es");
    pub const ITALIAN: Locale = Locale::two_letter(*b"it");

    const fn two_letter(code: [u8; 2]) -> Self {
        Locale {
            subtag: [code[0], code[1], 0],
            len: 2,
        }
    }

    /// The ISO 639 language subtag, such as `en` or `fil`.
    pub fn code(&self) -> &str {
        std::str::from_utf8(&self.subtag[..usize::from(self.len)]).unwrap_or("en")
    }

    /// Parse a locale from a language tag by its language subtag, so a
    /// regional tag Core has no copy for (`fr-CH`, POSIX `de_LI.UTF-8`)
    /// renders its language. The subtag must be 2–3 ASCII letters.
    pub fn from_code(tag: &str) -> Option<Self> {
        if tag.len() > MAX_TAG_BYTES {
            return None;
        }
        let language = tag.split(['-', '_', '.']).next().unwrap_or_default();
        if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic())
        {
            return None;
        }
        let mut subtag = [0; 3];
        for (slot, byte) in subtag.iter_mut().zip(language.bytes()) {
            *slot = byte.to_ascii_lowercase();
        }
        Some(Locale {
            subtag,
            len: language.len() as u8,
        })
    }
}

impl Default for Locale {
    fn default() -> Self {
        Locale::ENGLISH
    }
}

impl std::fmt::Debug for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Locale").field(&self.code()).finish()
    }
}

impl Serialize for Locale {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Locale {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tag = String::deserialize(deserializer)?;
        Locale::from_code(&tag)
            .ok_or_else(|| serde::de::Error::custom("not an ISO 639 language subtag"))
    }
}

/// Information about a locale
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleInfo {
    pub code: String,
    pub name: String,
    pub english_name: String,
    pub is_rtl: bool,
}

/// Get information about a locale: from its loaded file's `_meta` when
/// that is usable (for English, also the copy Core embeds), else by its
/// code (vauchi/private#733 D2, G2).
pub fn get_locale_info(locale: Locale) -> LocaleInfo {
    loaded_locale_info(locale)
        .or_else(|| listed_locale_info(locale))
        .or_else(|| {
            (locale == Locale::ENGLISH)
                .then(bundled_english_info)
                .flatten()
        })
        .unwrap_or_else(|| LocaleInfo {
            code: locale.code().to_string(),
            name: locale.code().to_string(),
            english_name: locale.code().to_string(),
            is_rtl: false,
        })
}

fn bundled_english_info() -> Option<LocaleInfo> {
    static INFO: std::sync::OnceLock<Option<LocaleInfo>> = std::sync::OnceLock::new();
    INFO.get_or_init(|| {
        parse_locale_file("en", BUNDLED_EN_JSON.as_bytes())
            .ok()
            .and_then(|(_, info)| info)
    })
    .clone()
}

fn loaded_locale_info(locale: Locale) -> Option<LocaleInfo> {
    LOCALE_META
        .read()
        .ok()?
        .as_ref()?
        .get(locale.code())
        .cloned()
}

/// The languages every shell bundles a file for. They resolve before the
/// shell has loaded its files, so a screen built at start renders in them.
const SHELL_BUNDLED_LOCALES: [Locale; 5] = [
    Locale::ENGLISH,
    Locale::GERMAN,
    Locale::FRENCH,
    Locale::SPANISH,
    Locale::ITALIAN,
];

/// The locale `tag` names, if Core can render it: a shell-bundled
/// language, or one whose file has loaded. Another well-formed tag yields
/// `None` until its file arrives, so a resolution chain falls through to
/// its next tag instead of rendering English over a language the person
/// reads (vauchi/private#733 D4).
pub fn translated_locale(tag: &str) -> Option<Locale> {
    Locale::from_code(tag)
        .filter(|locale| SHELL_BUNDLED_LOCALES.contains(locale) || has_loaded_strings(*locale))
}

fn has_loaded_strings(locale: Locale) -> bool {
    LOCALE_STORE
        .read()
        .map(|lock| {
            lock.as_ref()
                .is_some_and(|store| store.contains_key(locale.code()))
        })
        .unwrap_or(false)
}

/// The languages Settings offers: English, every language whose loaded
/// file has a usable `_meta`, and every language the trusted manifest
/// lists, before its file arrives (vauchi/private#733 D3). English leads,
/// the rest follow by code. A right-to-left language is left out while no
/// frontend mirrors its layout (the RTL canary in i18n_integration_tests).
pub fn get_available_locales() -> Vec<Locale> {
    let loaded: Vec<Locale> = LOCALE_META
        .read()
        .ok()
        .and_then(|lock| {
            lock.as_ref()
                .map(|meta| meta.keys().filter_map(|code| exact_locale(code)).collect())
        })
        .unwrap_or_default();
    let listed: Vec<Locale> = LISTED_LOCALES
        .read()
        .map(|lock| lock.iter().map(|(locale, _)| *locale).collect())
        .unwrap_or_default();
    let mut others: Vec<Locale> = loaded
        .into_iter()
        .chain(listed)
        .filter(|&locale| locale != Locale::ENGLISH && !get_locale_info(locale).is_rtl)
        .collect();
    others.sort_by(|a, b| a.code().cmp(b.code()));
    others.dedup();
    std::iter::once(Locale::ENGLISH).chain(others).collect()
}

/// A language the trusted manifest lists, with the names it gives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListedLocale {
    pub code: String,
    pub name: Option<String>,
    pub english_name: Option<String>,
    pub is_rtl: bool,
}

impl From<&str> for ListedLocale {
    fn from(code: &str) -> Self {
        ListedLocale {
            code: code.to_string(),
            ..ListedLocale::default()
        }
    }
}

/// The languages the trusted cached manifest lists, each with its names
/// when they pass the same checks as a file's `_meta`.
static LISTED_LOCALES: RwLock<Vec<(Locale, Option<LocaleInfo>)>> = RwLock::new(Vec::new());

/// Record the languages the trusted cached manifest lists, so Settings
/// offers and names them before their files arrive (vauchi/private#733 D3,
/// D7). An entry that is not exactly a language code is ignored, and
/// unusable names leave the language named by its code (DC-01).
pub fn set_listed_locales(entries: impl IntoIterator<Item = impl Into<ListedLocale>>) {
    let listed = entries
        .into_iter()
        .map(Into::into)
        .filter_map(|entry| {
            let locale = exact_locale(&entry.code)?;
            let info = match (entry.name, entry.english_name) {
                (Some(name), Some(english_name)) => RawLocaleMeta {
                    locale: entry.code,
                    name,
                    english_name,
                    is_rtl: entry.is_rtl,
                }
                .into_info(locale.code()),
                _ => None,
            };
            Some((locale, info))
        })
        .collect();
    if let Ok(mut lock) = LISTED_LOCALES.write() {
        *lock = listed;
    }
}

fn listed_locale_info(locale: Locale) -> Option<LocaleInfo> {
    LISTED_LOCALES
        .read()
        .ok()?
        .iter()
        .find(|(listed, _)| *listed == locale)
        .and_then(|(_, info)| info.clone())
}

/// The locale whose code is exactly `code`; a file or manifest entry named
/// `pt-BR` names no locale, since a locale holds only the language subtag.
fn exact_locale(code: &str) -> Option<Locale> {
    Locale::from_code(code).filter(|locale| locale.code() == code)
}

/// Get a localized string by key
pub fn get_string(locale: Locale, key: &str) -> String {
    if let Some(value) = lookup_one(locale, key) {
        return value;
    }

    // Fallback to English
    if locale != Locale::ENGLISH
        && let Some(value) = lookup_one(Locale::ENGLISH, key)
    {
        return value;
    }

    format!("Missing: {}", key)
}

/// The string for `key`, or `None` where neither the locale nor English
/// has one — for copy that is optional per screen, such as the context
/// bar's information text, where "Missing: …" must never reach a person.
pub fn try_get_string(locale: Locale, key: &str) -> Option<String> {
    lookup_one(locale, key).or_else(|| {
        (locale != Locale::ENGLISH)
            .then(|| lookup_one(Locale::ENGLISH, key))
            .flatten()
    })
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
    if locale == Locale::ENGLISH {
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

/// Each loaded file's usable `_meta`, by locale code. A file whose `_meta`
/// is unusable has strings in `LOCALE_STORE` and no entry here.
static LOCALE_META: RwLock<Option<HashMap<String, LocaleInfo>>> = RwLock::new(None);

/// Longest language name a `_meta` may carry; names are shown in a
/// dropdown, and a CDN file is untrusted input (DC-01).
const MAX_LANGUAGE_NAME_CHARS: usize = 64;

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
    let mut meta = HashMap::new();

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
                let (strings, info) = parse_locale_file(&code, &data)?;
                if let Some(info) = info {
                    meta.insert(code.clone(), info);
                }
                store.insert(code, strings);
            }
        }
    }

    *LOCALE_STORE.write().map_err(|_| I18nError::LockPoisoned)? = Some(store);
    *LOCALE_META.write().map_err(|_| I18nError::LockPoisoned)? = Some(meta);
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
    let (mut strings, info) = parse_locale_file(code, data)?;
    {
        let mut meta = LOCALE_META.write().map_err(|_| I18nError::LockPoisoned)?;
        let meta = meta.get_or_insert_with(HashMap::new);
        match info {
            Some(info) => meta.insert(code.to_string(), info),
            None => meta.remove(code),
        };
    }

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

/// Parse a locale file into its string map and, when usable, its `_meta`.
fn parse_locale_file(
    code: &str,
    data: &[u8],
) -> Result<(HashMap<String, String>, Option<LocaleInfo>), I18nError> {
    let mut raw: HashMap<String, serde_json::Value> = serde_json::from_slice(data)?;
    let info = raw
        .remove("_meta")
        .and_then(|meta| serde_json::from_value::<RawLocaleMeta>(meta).ok())
        .and_then(|meta| meta.into_info(code));
    let strings = raw
        .into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
        .collect();
    Ok((strings, info))
}

#[derive(Deserialize)]
struct RawLocaleMeta {
    locale: String,
    name: String,
    english_name: String,
    #[serde(default)]
    is_rtl: bool,
}

impl RawLocaleMeta {
    /// The file counts only under its own name, so a file cannot claim to
    /// be another language, and only with names a dropdown can show.
    fn into_info(self, file_code: &str) -> Option<LocaleInfo> {
        let usable = |name: &str| {
            (1..=MAX_LANGUAGE_NAME_CHARS).contains(&name.chars().count())
                && !name.chars().any(char::is_control)
        };
        let counts = self.locale == file_code && usable(&self.name) && usable(&self.english_name);
        counts.then_some(LocaleInfo {
            code: self.locale,
            name: self.name,
            english_name: self.english_name,
            is_rtl: self.is_rtl,
        })
    }
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
    if locale == Locale::ENGLISH {
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

/// The sibling `locales` checkout for unit tests. Prefers
/// `VAUCHI_LOCALES_DIR` because cargo-mutants builds a temp copy where the
/// relative sibling does not exist.
#[cfg(test)]
pub(crate) fn test_locales_dir() -> std::path::PathBuf {
    std::env::var_os("VAUCHI_LOCALES_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales")
        })
}

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
        let locales_dir = test_locales_dir();
        let _ = init(&locales_dir);
    }

    /// Helper: fully clear the store (only for tests that specifically test uninitialized state)
    fn clear_store() {
        let mut lock = LOCALE_STORE.write().unwrap_or_else(|e| e.into_inner());
        *lock = None;
    }

    #[test]
    fn test_locale_default() {
        assert_eq!(Locale::default(), Locale::ENGLISH);
    }

    #[test]
    fn test_locale_codes() {
        assert_eq!(Locale::ENGLISH.code(), "en");
        assert_eq!(Locale::GERMAN.code(), "de");
    }

    #[test]
    fn test_locale_from_code() {
        assert_eq!(Locale::from_code("en"), Some(Locale::ENGLISH));
        assert_eq!(Locale::from_code("EN"), Some(Locale::ENGLISH));
        assert_eq!(Locale::from_code("en-US"), Some(Locale::ENGLISH));
        assert_eq!(Locale::from_code("x"), None);
    }

    #[test]
    fn test_init_loads_locales() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        assert!(is_initialized());
        let s = get_string(Locale::ENGLISH, "welcome.title");
        assert_eq!(s, "Welcome to Vauchi");
    }

    #[test]
    fn test_init_german_strings() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let s = get_string(Locale::GERMAN, "welcome.title");
        assert_eq!(s, "Willkommen bei Vauchi");
    }

    #[test]
    fn test_fallback_to_english() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        // "update.sent" only exists in en, not de
        let de = get_string(Locale::GERMAN, "update.sent");
        let en = get_string(Locale::ENGLISH, "update.sent");
        assert_eq!(de, en);
    }

    #[test]
    fn test_get_string_missing() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let s = get_string(Locale::ENGLISH, "nonexistent");
        assert!(s.contains("Missing"));
    }

    #[test]
    fn test_interpolation() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let s = get_string_with_args(Locale::ENGLISH, "contacts.count", &[("count", "5")]);
        assert_eq!(s, "5 contacts");
    }

    #[test]
    fn the_bundled_locale_files_are_offered_with_english_first() {
        let _guard = store_guard();
        reset_store();

        let codes: Vec<String> = get_available_locales()
            .iter()
            .map(|l| l.code().to_string())
            .collect();

        assert_eq!(codes, ["en", "de", "es", "fr", "it"]);
    }

    #[test]
    fn test_meta_key_excluded() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let strings = get_strings_for_locale(Locale::ENGLISH);
        assert!(!strings.contains_key("_meta"));
    }

    #[test]
    fn test_reload_locale_updates_strings() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        assert_eq!(
            get_string(Locale::ENGLISH, "welcome.title"),
            "Welcome to Vauchi"
        );

        // Reload with updated strings
        let updated = serde_json::json!({
            "welcome.title": "Welcome Back to Vauchi"
        });
        load_locale_from_bytes("en", updated.to_string().as_bytes()).unwrap();

        assert_eq!(
            get_string(Locale::ENGLISH, "welcome.title"),
            "Welcome Back to Vauchi"
        );
    }

    #[test]
    fn test_bundled_english_fallback() {
        let _guard = store_guard();
        reset_store();
        // Without init(), should fall back to bundled_english
        let s = get_string(Locale::ENGLISH, "app.name");
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
            "nav.my_card",
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
                        let _ = get_string(Locale::ENGLISH, "welcome.title");
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
        let parent = TempDir::new().expect("temporary parent");
        let nonexistent = parent.path().join("missing");

        init(&nonexistent).expect("nonexistent resource directory");

        assert_eq!(get_string(Locale::ENGLISH, "app.name"), "Vauchi");
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
        assert_eq!(get_string(Locale::ENGLISH, "app.name"), "Test");
        // Restore for other tests
        reset_store();
    }

    #[test]
    fn test_all_strings_returns_full_map() {
        let _guard = store_guard();
        reset_store();
        let dir = setup_test_locales();
        init(dir.path()).unwrap();

        let all = get_all_strings(Locale::ENGLISH);
        assert!(all.contains_key("welcome.title"));
        assert!(all.contains_key("contacts.count"));
        assert!(!all.contains_key("_meta"));
    }
}
