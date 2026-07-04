// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S4b-3 (`2026-07-03-core-screens-bypass-i18n`): the BLE exchange
//! chrome (per-mode discovering / exchanging / verifying) and the Glance
//! active screen render in the user's locale. Keys in `exchange.ble.*`
//! (locales!88). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{BleExchangeEngine, ScreenModel, WorkflowEngine};
use vauchi_core::exchange::mode::ExchangeMode;

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

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
fn ble_discovering_renders_german() {
    load_german();
    let engine = bump_engine(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_ble_discovering");
    // Bump mode's per-mode discovering copy.
    assert_eq!(screen.title, "Bereit zum Anstoßen");
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("Stoßen Sie die Telefone zum Austausch aneinander")
    );
    // The scanning status line is localized too.
    let text = text_of(&screen);
    assert!(
        text.contains("Suche in der Nähe") || text.contains("Suche nach Geräten"),
        "scanning status localized; got {text}"
    );
}

// English stays exactly as before (regression pin).
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
