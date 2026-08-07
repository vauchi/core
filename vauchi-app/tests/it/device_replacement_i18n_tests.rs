// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the device-replacement
//! select-mode screen renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{DeviceReplacementEngine, WorkflowEngine};

/// `(title, has-old-device action label)` for the select-mode screen.
fn select_mode_copy(locale: Locale) -> (String, String) {
    let engine = DeviceReplacementEngine::new_target().with_locale(locale);
    let screen = engine.current_screen();
    (
        screen.title.clone(),
        action_label(&screen, "has_old_device"),
    )
}

// @scenario: device-replacement :: select-mode screen renders in the active locale
// @internal
#[test]
fn device_replacement_select_mode_renders_the_active_locale() {
    load_german();
    let (de_title, de_action) = select_mode_copy(Locale::German);
    let (en_title, en_action) = select_mode_copy(Locale::English);

    assert_translated("select-mode title", &de_title, &en_title);
    assert_translated("has-old-device action", &de_action, &en_action);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn device_replacement_select_mode_english_copy_unchanged() {
    let engine = DeviceReplacementEngine::new_target();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Transfer from another device");
    assert_eq!(action_label(&screen, "has_old_device"), "Transfer via QR");
}
