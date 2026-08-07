// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the create-group form
//! dialog renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{FormDialogEngine, FormDialogType, WorkflowEngine};

/// `(title, submit action label)` for the create-group dialog.
fn create_group_copy(locale: Locale) -> (String, String) {
    let engine = FormDialogEngine::new(FormDialogType::CreateGroup).with_locale(locale);
    let screen = engine.current_screen();
    (screen.title.clone(), action_label(&screen, "submit"))
}

// @scenario: form-dialog :: create-group screen renders in the active locale
// @internal
#[test]
fn form_dialog_create_group_renders_the_active_locale() {
    load_german();
    let (de_title, de_submit) = create_group_copy(Locale::German);
    let (en_title, en_submit) = create_group_copy(Locale::English);

    assert_translated("create-group title", &de_title, &en_title);
    assert_translated("submit action", &de_submit, &en_submit);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn form_dialog_create_group_english_copy_unchanged() {
    let engine = FormDialogEngine::new(FormDialogType::CreateGroup);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "New Group");
    assert_eq!(action_label(&screen, "submit"), "Create");
}
