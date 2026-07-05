// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-11 (`2026-07-03-core-screens-bypass-i18n`): the link-mode
//! initiator screens render in the user's locale. Keys in
//! `link_exchange.*` (locales!108). Exact German assertions per
//! CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{LinkExchangeEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: link-exchange :: share-url screen renders in the active locale
// @internal
#[test]
fn link_exchange_share_url_screen_renders_german() {
    load_german();
    let engine = LinkExchangeEngine::new().with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Link teilen");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "share")
            .unwrap()
            .label,
        "Teilen"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn link_exchange_share_url_screen_english_copy_unchanged() {
    let engine = LinkExchangeEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Share Link");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "share")
            .unwrap()
            .label,
        "Share"
    );
}
