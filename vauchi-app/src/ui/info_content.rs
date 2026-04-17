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

    #[test]
    fn resolve_known_key_returns_content() {
        let result = resolve_info_key("auto_lock", Locale::English);
        assert!(result.is_some(), "expected Some for known key 'auto_lock'");
        let (title, body) = result.unwrap();
        assert!(!title.is_empty(), "title must not be empty");
        assert!(!body.is_empty(), "body must not be empty");
    }

    #[test]
    fn resolve_unknown_key_returns_none() {
        let result = resolve_info_key("nonexistent_key_xyz", Locale::English);
        assert!(result.is_none(), "expected None for unknown key");
    }

    #[test]
    fn resolve_known_key_has_non_missing_content() {
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
