// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-9 (`2026-07-03-core-screens-bypass-i18n`): the device
//! replacement wizard renders in the user's locale. Keys in
//! `device.*` (locales!106). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{DeviceReplacementEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: device-replacement :: select-mode screen renders in the active locale
// @internal
#[test]
fn device_replacement_select_mode_renders_german() {
    load_german();
    let engine = DeviceReplacementEngine::new_target().with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Von einem anderen Gerät übertragen");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "has_old_device")
            .unwrap()
            .label,
        "Per QR übertragen"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn device_replacement_select_mode_english_copy_unchanged() {
    let engine = DeviceReplacementEngine::new_target();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Transfer from another device");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "has_old_device")
            .unwrap()
            .label,
        "Transfer via QR"
    );
}
