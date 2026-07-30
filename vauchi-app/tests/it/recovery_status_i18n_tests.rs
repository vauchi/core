// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-2 (`2026-07-03-core-screens-bypass-i18n`): the outgoing
//! social-recovery screens render in the user's locale. Keys in
//! `recovery.*` (locales!101). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{Component, RecoveryEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: recovery :: intro screen renders in the active locale
// @internal
#[test]
fn recovery_intro_screen_renders_german() {
    load_german();
    let engine = RecoveryEngine::new(vec![], 3).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Soziale Wiederherstellung");
    let Component::InfoPanel { title, .. } = &screen.components[0] else {
        panic!("expected InfoPanel, got {:?}", screen.components[0]);
    };
    assert_eq!(title, "Gerät verloren?");
    assert_eq!(
        screen.contextual_actions[0].label,
        "Wiederherstellungsprozess starten"
    );
}

// English stays exactly as before (regression pin).
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
