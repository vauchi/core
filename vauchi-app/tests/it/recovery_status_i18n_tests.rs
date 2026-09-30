// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the social-recovery intro
//! screen renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{Component, RecoveryEngine, WorkflowEngine};

/// `(title, intro text, Start recovery label)` for the intro screen.
fn intro_copy(locale: Locale) -> (String, String, String) {
    let engine = RecoveryEngine::new(vec![], 3).with_locale(locale);
    let screen = engine.current_screen();
    let intro = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Text { id, content, .. } if id == "intro" => Some(content.clone()),
            _ => None,
        })
        .expect("intro text");
    let start = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ButtonList { items, .. } => items
                .iter()
                .find(|i| i.id == "start_recovery_process")
                .map(|i| i.label.clone()),
            _ => None,
        })
        .expect("Start recovery button");
    (screen.title.clone(), intro, start)
}

// @scenario: recovery :: intro screen renders in the active locale
// @internal
#[test]
fn recovery_intro_screen_renders_the_active_locale() {
    load_german();
    let (de_title, de_intro, de_start) = intro_copy(Locale::German);
    let (en_title, en_intro, en_start) = intro_copy(Locale::English);

    assert_translated("recovery intro title", &de_title, &en_title);
    assert_translated("recovery intro text", &de_intro, &en_intro);
    assert_translated("start-recovery button", &de_start, &en_start);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn recovery_intro_screen_english_copy_unchanged() {
    let (title, _, start) = intro_copy(Locale::English);
    assert_eq!(title, "Social Recovery");
    assert_eq!(start, "Start recovery");
}
