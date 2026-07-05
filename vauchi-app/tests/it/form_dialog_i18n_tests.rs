// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-7 (`2026-07-03-core-screens-bypass-i18n`): the generic form
//! dialog screens render in the user's locale. Keys in `form.*`
//! (locales!104). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{FormDialogEngine, FormDialogType, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: form-dialog :: create-group screen renders in the active locale
// @internal
#[test]
fn form_dialog_create_group_renders_german() {
    load_german();
    let engine = FormDialogEngine::new(FormDialogType::CreateGroup).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Neue Gruppe");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "submit")
            .unwrap()
            .label,
        "Erstellen"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn form_dialog_create_group_english_copy_unchanged() {
    let engine = FormDialogEngine::new(FormDialogType::CreateGroup);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "New Group");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "submit")
            .unwrap()
            .label,
        "Create"
    );
}
