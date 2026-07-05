// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-8 (`2026-07-03-core-screens-bypass-i18n`): the helper-side
//! (vouching) social-recovery screens render in the user's locale.
//! Keys in `recovery.*` (locales!105). Exact German assertions per
//! CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{RecoveryHelpEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: recovery-help :: info screen renders in the active locale
// @internal
#[test]
fn recovery_help_info_screen_renders_german() {
    load_german();
    let engine = RecoveryHelpEngine::new().with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Anderen helfen");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "vouch")
            .unwrap()
            .label,
        "Für jemanden bürgen"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn recovery_help_info_screen_english_copy_unchanged() {
    let engine = RecoveryHelpEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Help Others");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "vouch")
            .unwrap()
            .label,
        "Vouch for Someone"
    );
}
