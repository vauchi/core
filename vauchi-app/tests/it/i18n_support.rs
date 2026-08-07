// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared support for the locale-rendering tests.
//!
//! `load_german` was copy-pasted into 28 test files, and each carried its
//! own verbatim assertions about what the German copy says. That coupled
//! core to a repo it does not own: a register unification merged in
//! `locales` on 2026-08-07 reddened five of those files across every core
//! branch at once, with no commit in core.
//!
//! Record: `problems/2026-08-07-locale-content-consumed-from-unpinned-head/`

// Not every consumer of this module uses every helper.
#![allow(dead_code)]

use vauchi_app::i18n::load_locale_from_bytes;
use vauchi_app::ui::ScreenModel;

/// Loads the real German locale, exactly as CI does — the
/// `.clone-locales` template places the checkout as a sibling of core,
/// at the same relative path `build.rs` bundles English from.
pub fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

pub fn action_label(screen: &ScreenModel, id: &str) -> String {
    screen
        .contextual_actions
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("action {id} present"))
        .label
        .clone()
}

/// Asserts a rendered string is genuinely translated.
///
/// Non-emptiness alone is too weak to be a check: `strings_for` returns
/// an empty map for a non-English locale with no data, and the caller
/// then falls back to English, so an untranslated string is non-empty
/// and reads as healthy. Differing from the English render is what
/// makes this discriminating.
///
/// What the translation *says* is `locales`' business — it owns the
/// schema, the quality gates and the CODEOWNERS for that. Asserting the
/// copy here put the failure in the wrong repo, on every branch at once.
///
/// A string legitimately identical in both languages (a proper noun, a
/// symbol) cannot use this helper and needs an explicit, commented
/// exemption rather than a silent weakening.
pub fn assert_translated(field: &str, translated: &str, english: &str) {
    assert!(!translated.is_empty(), "{field}: rendered empty");
    assert!(
        !translated.starts_with("Missing:"),
        "{field}: key did not resolve, got {translated:?}"
    );
    assert_ne!(
        translated, english,
        "{field}: rendered the English copy, so no translation was applied \
         (an absent locale falls back to English rather than failing)"
    );
}

/// Asserts every paired field is translated. Pairs are
/// `(field name, translated render, English render)`.
pub fn assert_all_translated(pairs: &[(&str, String, String)]) {
    for (field, translated, english) in pairs {
        assert_translated(field, translated, english);
    }
}
