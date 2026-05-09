// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolves info_key values to localized help content.

use crate::i18n::{Locale, get_string};

/// Resolve an info_key to (title, body) using locale keys
/// `info.{key}.title` and `info.{key}.body`.
///
/// Returns `None` if either key is missing from the locale.
pub fn resolve_info_key(key: &str, locale: Locale) -> Option<(String, String)> {
    let title_key = format!("info.{key}.title");
    let body_key = format!("info.{key}.body");
    let title = get_string(locale, &title_key);
    let body = get_string(locale, &body_key);
    // get_string returns "Missing: {key}" if the key is absent
    if title.starts_with("Missing:") || body.starts_with("Missing:") {
        return None;
    }
    Some((title, body))
}

// INLINE_TEST_REQUIRED: tests depend on the bundled locale state via get_string
#[cfg(test)]
mod tests {
    use super::*;

    // Self-initializing i18n setup. Without this, single-crate runs
    // (just test-crate vauchi-app) fail because no other test in the
    // same crate initializes the locale store before these run; the
    // workspace path (just test core) happens to pass via test ordering.
    //
    // Why always re-init (vs help.rs pattern of "init only if not
    // initialized"): is_initialized() returns true whenever
    // LOCALE_STORE is Some(_), but several i18n tests
    // (test_concurrent_read_during_reload, test_init_with_nonexistent_dir)
    // replace the store with a stripped-down test set covering only
    // welcome.title etc. and do not restore on drop. is_initialized
    // remains true but the store no longer contains info.auto_lock.title.
    // Always re-loading the production locales dir under the
    // serializing I18N_TEST_LOCK is the only race-free guarantee that
    // these tests' keys are present.
    fn setup_i18n() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::i18n::I18N_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let locales_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        let _ = crate::i18n::init(&locales_dir);
        guard
    }

    // @internal
    #[test]
    fn resolve_known_key_returns_content() {
        let _g = setup_i18n();
        let result = resolve_info_key("auto_lock", Locale::English);
        assert!(result.is_some(), "expected Some for known key 'auto_lock'");
        let (title, body) = result.unwrap();
        assert!(!title.is_empty(), "title must not be empty");
        assert!(!body.is_empty(), "body must not be empty");
    }

    // @internal
    #[test]
    fn resolve_unknown_key_returns_none() {
        let _g = setup_i18n();
        let result = resolve_info_key("nonexistent_key_xyz", Locale::English);
        assert!(result.is_none(), "expected None for unknown key");
    }

    // @internal
    #[test]
    fn resolve_known_key_has_non_missing_content() {
        let _g = setup_i18n();
        let (title, body) = resolve_info_key("auto_lock", Locale::English).unwrap();
        assert!(
            !title.starts_with("Missing:"),
            "title should not be a missing-key placeholder"
        );
        assert!(
            !body.starts_with("Missing:"),
            "body should not be a missing-key placeholder"
        );
    }
}
