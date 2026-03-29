// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! C ABI wrappers for the i18n (internationalization) system.
//!
//! Provides translated strings to C consumers (Linux-Qt via vauchi.h).
//! Locale JSON files must be loaded via `vauchi_i18n_init` before
//! `vauchi_i18n_get_string` returns runtime-loaded translations;
//! without init, English falls back to the compile-time bundled set.

use std::os::raw::c_char;
use std::path::Path;

use super::{from_c_str, to_c_string};

/// Get a translated string for the given locale and key.
///
/// Returns the translated string, falling back to English if the
/// locale lacks the key, or `"Missing: <key>"` if no translation
/// exists at all. Returns null if `locale_code` or `key` is null,
/// or `locale_code` is not a recognised locale.
///
/// The caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `locale_code` and `key` must be valid null-terminated C strings,
/// or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_i18n_get_string(
    locale_code: *const c_char,
    key: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let code = match from_c_str(locale_code) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let key = match from_c_str(key) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let locale = match vauchi_app::i18n::Locale::from_code(&code) {
            Some(l) => l,
            None => return std::ptr::null_mut(),
        };
        let translated = vauchi_app::i18n::get_string(locale, &key);
        to_c_string(&translated)
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Return a JSON array of all available locale codes.
///
/// Example: `["en","de","fr","es","it"]`
///
/// The caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// No special requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_i18n_available_locales() -> *mut c_char {
    match std::panic::catch_unwind(|| {
        let codes: Vec<&str> = vauchi_app::i18n::get_available_locales()
            .iter()
            .map(|l| l.code())
            .collect();
        let json = serde_json::json!(codes).to_string();
        to_c_string(&json)
    }) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Initialise the i18n system from a directory of JSON locale files.
///
/// Each `*.json` file in `resource_dir` is loaded as a locale
/// (filename stem = locale code, e.g. `de.json` → `"de"`).
///
/// Returns 0 on success, 1 on failure.
///
/// # Safety
/// `resource_dir` must be a valid null-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_i18n_init(resource_dir: *const c_char) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir = match from_c_str(resource_dir) {
            Some(s) => s,
            None => return 1,
        };
        match vauchi_app::i18n::init(Path::new(&dir)) {
            Ok(()) => 0,
            Err(_) => 1,
        }
    }))
    .unwrap_or(1)
}

/// Check whether the i18n system has been initialised.
///
/// Returns 1 if at least one locale has been loaded, 0 otherwise.
///
/// # Safety
/// No special requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_i18n_is_initialized() -> i32 {
    std::panic::catch_unwind(|| vauchi_app::i18n::is_initialized() as i32).unwrap_or_default()
}
