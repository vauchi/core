// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the More menu renders in
//! the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{Component, MoreEngine, WorkflowEngine};

/// `(title, primary section, settings item, secondary section, recovery item)`.
fn more_copy(locale: Locale) -> (String, String, String, String, String) {
    let engine = MoreEngine::new(locale);
    let screen = engine.current_screen();

    let Component::SectionedActionList { sections, .. } = &screen.components[0] else {
        panic!("More renders a SectionedActionList");
    };
    let primary = sections
        .iter()
        .find(|s| s.id == "primary")
        .expect("primary section present");
    let settings_item = primary
        .items
        .iter()
        .find(|i| i.id == "settings")
        .expect("settings item present");
    let secondary = sections
        .iter()
        .find(|s| s.id == "secondary")
        .expect("secondary section present");
    let recovery_item = secondary
        .items
        .iter()
        .find(|i| i.id == "recovery")
        .expect("recovery item present");

    (
        screen.title.clone(),
        primary.label.clone(),
        settings_item.label.clone(),
        secondary.label.clone(),
        recovery_item.label.clone(),
    )
}

// @scenario: navigation :: More menu renders in the active locale
// @internal
#[test]
fn more_menu_renders_the_active_locale() {
    load_german();
    let (de_title, de_primary, de_settings, de_secondary, de_recovery) = more_copy(Locale::German);
    let (en_title, en_primary, en_settings, en_secondary, en_recovery) = more_copy(Locale::English);

    assert_translated("More title", &de_title, &en_title);
    assert_translated("settings item", &de_settings, &en_settings);
    assert_translated("secondary section label", &de_secondary, &en_secondary);
    assert_translated("recovery item", &de_recovery, &en_recovery);

    // Exemption: the primary section is labelled "App" in both locales —
    // the word is identical in German, so it cannot be asserted as
    // translated. Pinned exactly instead.
    assert_eq!(de_primary, "App");
    assert_eq!(de_primary, en_primary);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
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
