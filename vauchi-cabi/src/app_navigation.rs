// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Navigation C ABI — exposes `AppEngine::tab_info(locale)` and
//! `AppEngine::sidebar_items(locale)` so linux-qt / Windows can drive
//! their tab-bar and sidebar rendering off core instead of maintaining
//! parallel `AppScreen`→label match tables (§6 pure-renderer
//! remediation).

use std::os::raw::c_char;

use vauchi_app::i18n::Locale;
use vauchi_app::ui::TabInfo;

use super::{VauchiApp, from_c_str, to_c_string};

/// Serialize a `TabInfo` slice as a JSON array. Each entry has the
/// same shape as the UniFFI `MobileTabInfo` record:
/// `{id, label, icon, badge_count}`.
pub(super) fn tabs_to_json(tabs: &[TabInfo]) -> String {
    let items: Vec<serde_json::Value> = tabs
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "action_id": t.action_id,
                "label": t.label,
                "icon": t.icon,
                "badge_count": t.badge_count,
            })
        })
        .collect();
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
}

/// Resolve a C locale code to a `Locale`, falling back to the default
/// when the string is null, malformed, or unrecognised.
///
/// # Safety
/// `locale_code` must be a valid null-terminated C string or null.
pub(super) unsafe fn resolve_locale(locale_code: *const c_char) -> Locale {
    // SAFETY: forwards the caller's C-string invariant to `from_c_str`,
    // which handles null and non-UTF-8 gracefully.
    let code = from_c_str(locale_code);
    code.as_deref()
        .and_then(Locale::from_code)
        .unwrap_or_default()
}

/// Return the mobile tab-bar metadata as a JSON array.
///
/// Mirrors `PlatformAppEngine::tab_info(MobileLocale)` (UniFFI) — each
/// element is `{id, label, icon, badge_count}`. Pre-identity the
/// result is a single-element `[{onboarding ...}]` array; post-identity
/// it is the 5-element bottom-tab set (MyInfo / Contacts / Exchange /
/// Groups / More). Labels are pre-localized via core i18n, with an
/// English fallback when the key is missing.
///
/// `locale_code` is a null-terminated ISO code (`"en"`, `"de"`, ...).
/// Null, malformed, or unknown codes fall back to the default locale.
///
/// Returns null when `handle` is null. The caller must free the
/// returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid app handle or null. `locale_code` must be
/// a valid null-terminated C string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_tab_info(
    handle: *mut VauchiApp,
    locale_code: *const c_char,
) -> *mut c_char {
    // SAFETY: handle is checked non-null; ptr was created by Box::into_raw and has not been freed.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            let locale = resolve_locale(locale_code);
            match app.engine.lock() {
                Ok(engine) => to_c_string(&tabs_to_json(&engine.tab_info(locale))),
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Return the desktop sidebar metadata as a JSON array.
///
/// Mirrors `PlatformAppEngine::sidebar_items(MobileLocale)` (UniFFI) —
/// the broader 14-entry top-level set used by desktop frames
/// (MyInfo, Contacts, Exchange, Groups, Settings, Recovery,
/// DeviceManagement, Backup, Privacy, Support, Help, ActivityLog,
/// Sync, More). Pre-identity returns a single-element `[{onboarding}]`
/// array.
///
/// `locale_code`: see `vauchi_app_tab_info`.
///
/// # Safety
/// Same as `vauchi_app_tab_info`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_sidebar_items(
    handle: *mut VauchiApp,
    locale_code: *const c_char,
) -> *mut c_char {
    // SAFETY: handle is checked non-null; ptr was created by Box::into_raw and has not been freed.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            let locale = resolve_locale(locale_code);
            match app.engine.lock() {
                Ok(engine) => to_c_string(&tabs_to_json(&engine.sidebar_items(locale))),
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

// INLINE_TEST_REQUIRED: cdylib crate-type prevents integration tests in tests/ directory
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn tabs_to_json_serializes_all_fields() {
        let tabs = vec![TabInfo {
            id: "contacts".into(),
            action_id: "contacts".into(),
            label: "Contacts".into(),
            icon: "person.2".into(),
            badge_count: 3,
            is_home: false,
        }];
        let json = tabs_to_json(&tabs);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0]["id"], "contacts");
        assert_eq!(parsed[0]["action_id"], "contacts");
        assert_eq!(parsed[0]["label"], "Contacts");
        assert_eq!(parsed[0]["icon"], "person.2");
        assert_eq!(parsed[0]["badge_count"], 3);
    }

    // @internal
    #[test]
    fn tabs_to_json_empty_produces_empty_array() {
        assert_eq!(tabs_to_json(&[]), "[]");
    }

    // @internal
    #[test]
    fn tabs_to_json_preserves_order() {
        let tabs = vec![
            TabInfo {
                id: "a".into(),
                action_id: "a".into(),
                label: "A".into(),
                icon: "".into(),
                badge_count: 0,
                is_home: false,
            },
            TabInfo {
                id: "b".into(),
                action_id: "b".into(),
                label: "B".into(),
                icon: "".into(),
                badge_count: 0,
                is_home: false,
            },
        ];
        let json = tabs_to_json(&tabs);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0]["id"], "a");
        assert_eq!(parsed[1]["id"], "b");
    }

    // @internal
    #[test]
    fn resolve_locale_null_returns_default() {
        // SAFETY: null pointer is a documented accepted input.
        let locale = unsafe { resolve_locale(std::ptr::null()) };
        assert_eq!(locale, Locale::default());
    }

    // @internal
    #[test]
    fn resolve_locale_unknown_code_returns_default() {
        let c = std::ffi::CString::new("zz").unwrap();
        // SAFETY: `c` is a valid NUL-terminated C string that outlives the call.
        let locale = unsafe { resolve_locale(c.as_ptr()) };
        assert_eq!(locale, Locale::default());
    }

    // @internal
    #[test]
    fn resolve_locale_known_code_returns_matching_locale() {
        let c = std::ffi::CString::new("de").unwrap();
        // SAFETY: `c` is a valid NUL-terminated C string that outlives the call.
        let locale = unsafe { resolve_locale(c.as_ptr()) };
        assert_eq!(locale, Locale::from_code("de").unwrap());
    }
}
