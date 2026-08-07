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

/// `(title, InfoPanel title, primary action label)` for the intro screen.
fn intro_copy(locale: Locale) -> (String, String, String) {
    let engine = RecoveryEngine::new(vec![], 3).with_locale(locale);
    let screen = engine.current_screen();
    let Component::InfoPanel { title, .. } = &screen.components[0] else {
        panic!("expected InfoPanel, got {:?}", screen.components[0]);
    };
    (
        screen.title.clone(),
        title.clone(),
        screen.contextual_actions[0].label.clone(),
    )
}

// @scenario: recovery :: intro screen renders in the active locale
// @internal
#[test]
fn recovery_intro_screen_renders_the_active_locale() {
    load_german();
    let (de_title, de_panel, de_action) = intro_copy(Locale::German);
    let (en_title, en_panel, en_action) = intro_copy(Locale::English);

    assert_translated("recovery intro title", &de_title, &en_title);
    assert_translated("lost-device panel title", &de_panel, &en_panel);
    assert_translated("start-recovery action", &de_action, &en_action);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn recovery_intro_screen_english_copy_unchanged() {
    let engine = RecoveryEngine::new(vec![], 3);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Social Recovery");
    let Component::InfoPanel { title, .. } = &screen.components[0] else {
        panic!("expected InfoPanel, got {:?}", screen.components[0]);
    };
    assert_eq!(title, "Lost Your Device?");
    assert_eq!(screen.contextual_actions[0].label, "Start Recovery Process");
}
