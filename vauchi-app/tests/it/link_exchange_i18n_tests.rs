// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the link-exchange
//! share-url screen renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{LinkExchangeEngine, WorkflowEngine};

/// `(title, share action label)` for the share-url screen.
fn share_copy(locale: Locale) -> (String, String) {
    let engine = LinkExchangeEngine::new().with_locale(locale);
    let screen = engine.current_screen();
    (screen.title.clone(), action_label(&screen, "share"))
}

// @scenario: link-exchange :: share-url screen renders in the active locale
// @internal
#[test]
fn link_exchange_share_url_screen_renders_the_active_locale() {
    load_german();
    let (de_title, de_share) = share_copy(Locale::German);
    let (en_title, en_share) = share_copy(Locale::English);

    assert_translated("share-url title", &de_title, &en_title);
    assert_translated("share action", &de_share, &en_share);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn link_exchange_share_url_screen_english_copy_unchanged() {
    let engine = LinkExchangeEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Share Link");
    assert_eq!(action_label(&screen, "share"), "Share");
}
