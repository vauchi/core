// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the recovery-help info
//! screen renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{RecoveryHelpEngine, WorkflowEngine};

/// `(title, vouch action label)` for the help screen.
fn help_copy(locale: Locale) -> (String, String) {
    let engine = RecoveryHelpEngine::new().with_locale(locale);
    let screen = engine.current_screen();
    (screen.title.clone(), action_label(&screen, "vouch"))
}

// @scenario: recovery-help :: info screen renders in the active locale
// @internal
#[test]
fn recovery_help_info_screen_renders_the_active_locale() {
    load_german();
    let (de_title, de_vouch) = help_copy(Locale::German);
    let (en_title, en_vouch) = help_copy(Locale::English);

    assert_translated("recovery-help title", &de_title, &en_title);
    assert_translated("vouch action", &de_vouch, &en_vouch);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn recovery_help_info_screen_english_copy_unchanged() {
    let engine = RecoveryHelpEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Help Others");
    assert_eq!(action_label(&screen, "vouch"), "Vouch for Someone");
}
