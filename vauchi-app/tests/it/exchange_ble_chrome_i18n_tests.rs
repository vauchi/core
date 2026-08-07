// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S4b-3 (`2026-07-03-core-screens-bypass-i18n`): the BLE exchange
//! chrome (per-mode discovering / exchanging / verifying) and the Glance
//! active screen render in the user's locale. Keys in `exchange.ble.*`
//! (locales!88).
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{BleExchangeEngine, ScreenModel, WorkflowEngine};
use vauchi_core::exchange::mode::ExchangeMode;

fn bump_engine(locale: Locale) -> BleExchangeEngine {
    BleExchangeEngine::new(
        ExchangeMode::Bump,
        true,
        vec![],
        vauchi_core::clock::SystemClock::shared(),
        None,
        locale,
    )
}

fn text_of(screen: &ScreenModel) -> String {
    let mut out = vec![screen.title.clone()];
    if let Some(s) = &screen.subtitle {
        out.push(s.clone());
    }
    for c in &screen.components {
        if let vauchi_app::ui::Component::Text { content, .. } = c {
            out.push(content.clone());
        }
    }
    out.join(" | ")
}

// @scenario: exchange :: BLE discovering chrome renders in the active locale
// @internal
#[test]
fn ble_discovering_renders_the_active_locale() {
    load_german();
    let de = bump_engine(Locale::German).current_screen();
    let en = bump_engine(Locale::English).current_screen();

    // Screen ids are identifiers, not copy — they must NOT translate.
    assert_eq!(de.screen_id, "exchange_ble_discovering");
    assert_eq!(de.screen_id, en.screen_id);

    assert_translated("discovering title", &de.title, &en.title);
    assert_translated(
        "discovering subtitle",
        de.subtitle.as_deref().expect("subtitle present"),
        en.subtitle.as_deref().expect("subtitle present"),
    );
    // The scanning status line rides the same screen body.
    assert_translated("scanning status body", &text_of(&de), &text_of(&en));
}

// English stays exactly as before (regression pin). English is the source
// language and ships in this repo's bundled locale, so pinning it here
// couples nothing external.
// @internal
#[test]
fn ble_discovering_english_copy_unchanged() {
    let engine = bump_engine(Locale::English);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Ready to bump");
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("Bump your phones together to exchange")
    );
}
