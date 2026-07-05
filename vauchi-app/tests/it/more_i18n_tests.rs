// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6b-1 (`2026-07-03-core-screens-bypass-i18n`): the More-menu
//! sections and items render in the user's locale. Reuses existing
//! `nav.*` keys where an item already has one; new items get `more.*`
//! keys (locales!90). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{Component, MoreEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: navigation :: More menu renders in the active locale
// @internal
#[test]
fn more_menu_renders_german() {
    load_german();
    let engine = MoreEngine::new(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Mehr");

    let Component::SectionedActionList { sections, .. } = &screen.components[0] else {
        panic!("More renders a SectionedActionList");
    };
    let primary = sections
        .iter()
        .find(|s| s.id == "primary")
        .expect("primary section present");
    assert_eq!(primary.label, "App");
    let settings_item = primary
        .items
        .iter()
        .find(|i| i.id == "settings")
        .expect("settings item present");
    assert_eq!(settings_item.label, "Einstellungen");

    let secondary = sections
        .iter()
        .find(|s| s.id == "secondary")
        .expect("secondary section present");
    assert_eq!(secondary.label, "Konto & Geräte");
    let recovery_item = secondary
        .items
        .iter()
        .find(|i| i.id == "recovery")
        .expect("recovery item present");
    assert_eq!(recovery_item.label, "Soziale Wiederherstellung");
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn more_menu_english_copy_unchanged() {
    let engine = MoreEngine::new(Locale::English);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "More");

    let Component::SectionedActionList { sections, .. } = &screen.components[0] else {
        panic!("More renders a SectionedActionList");
    };
    let primary = sections.iter().find(|s| s.id == "primary").unwrap();
    assert_eq!(primary.label, "App");
    assert_eq!(
        primary
            .items
            .iter()
            .find(|i| i.id == "settings")
            .unwrap()
            .label,
        "Settings"
    );
}
