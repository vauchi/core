// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the contact-detail screen,
//! its actionable archive toast and the not-found screen render in the
//! user's locale.
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`. The undo
//! action id is pinned exactly: it is an identifier, not copy.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, ContactDetailEngine, ContactNotFoundEngine, Field, Item, UserAction,
    WorkflowEngine,
};

fn sample_contact() -> Item {
    Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: None,
        initials: "A".into(),
        status: None,
        actions: vec![],
        a11y: None,
    }
}

fn detail_engine(locale: Locale) -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), Vec::<Field>::new(), String::new())
        .with_locale(locale)
}

fn edit_label(locale: Locale) -> String {
    action_label(&detail_engine(locale).current_screen(), "edit")
}

/// `(undo action id, undo label)` from the archive toast.
fn archive_toast(locale: Locale) -> (String, String) {
    let mut engine = detail_engine(locale);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "archive_contact".into(),
    });
    match result {
        ActionResult::ShowToast {
            undo_action_id,
            undo_label,
            ..
        } => (
            undo_action_id.expect("actionable toast carries an undo id"),
            undo_label.expect("actionable toast carries an undo label"),
        ),
        other => panic!("expected actionable ShowToast, got {other:?}"),
    }
}

/// `(title, back action label)` for the not-found screen.
fn not_found_copy(locale: Locale) -> (String, String) {
    let engine = ContactNotFoundEngine::new("c1".into()).with_locale(locale);
    let screen = engine.current_screen();
    (screen.title.clone(), action_label(&screen, "back"))
}

// @scenario: contact-detail :: main screen renders in the active locale
// @internal
#[test]
fn contact_detail_renders_the_active_locale() {
    load_german();
    assert_translated(
        "edit action",
        &edit_label(Locale::German),
        &edit_label(Locale::English),
    );
}

// @scenario: contact-detail :: actionable toast copy is resolved by core
// @internal
#[test]
fn contact_detail_archive_toast_undo_label_is_translated() {
    load_german();
    let (de_id, de_label) = archive_toast(Locale::German);
    let (en_id, en_label) = archive_toast(Locale::English);

    // The undo action id is an identifier, not copy — it must NOT
    // translate, and it must carry the contact it undoes.
    assert_eq!(de_id, "undo_archive_contact:c1");
    assert_eq!(de_id, en_id);

    assert_translated("archive-toast undo label", &de_label, &en_label);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn contact_detail_english_copy_unchanged() {
    assert_eq!(edit_label(Locale::English), "Edit");
}

// @scenario: contact-detail :: not-found screen renders in the active locale
// @internal
#[test]
fn contact_not_found_renders_the_active_locale() {
    load_german();
    let (de_title, de_back) = not_found_copy(Locale::German);
    let (en_title, en_back) = not_found_copy(Locale::English);

    assert_translated("not-found title", &de_title, &en_title);
    assert_translated("back action", &de_back, &en_back);
}

// @internal
#[test]
fn contact_not_found_english_copy_unchanged() {
    let (title, back) = not_found_copy(Locale::English);
    assert_eq!(title, "Contact Not Found");
    assert_eq!(back, "Back");
}
