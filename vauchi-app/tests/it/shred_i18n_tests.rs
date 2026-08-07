// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S3 (`2026-07-03-core-screens-bypass-i18n`, design D3.2): the
//! emergency-wipe wizard renders in the user's locale, threaded from the
//! engine entry point — the first of the destructive/security confirmation
//! screens to leave hardcoded English behind. Keys live in the
//! `shred.wipe.*` family (locales!80).
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{ActionResult, Component, EmergencyShredEngine, UserAction, WorkflowEngine};

/// Copy a shell would show walking the wipe wizard to its validation error.
struct ShredCopy {
    warning_screen_id: String,
    warning_title: String,
    continue_action: String,
    cancel_action: String,
    first_consequence: String,
    irreversible_detail: String,
    confirm_title: String,
    confirmation_label: String,
    wipe_action: String,
    empty_confirmation_error: String,
}

fn walk_shred(locale: Locale) -> ShredCopy {
    let mut engine = EmergencyShredEngine::new(locale);

    let warning = engine.current_screen();
    let warning_screen_id = warning.screen_id.clone();
    let warning_title = warning.title.clone();
    let continue_action = action_label(&warning, "continue");
    let cancel_action = action_label(&warning, "cancel");
    let Component::InfoPanel { items, .. } = &warning.components[0] else {
        panic!("warning screen leads with the consequences InfoPanel");
    };
    let first_consequence = items[0].title.clone();
    let irreversible_detail = items[2].detail.clone();

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let confirm = engine.current_screen();
    let confirm_title = confirm.title.clone();
    let Component::TextInput { label, .. } = &confirm.components[0] else {
        panic!("confirm screen leads with the typed-confirmation input");
    };
    let confirmation_label = label.clone();
    let wipe_action = action_label(&confirm, "wipe");

    // The wrong-text validation message is localized too.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });
    let ActionResult::ValidationError { message, .. } = result else {
        panic!("empty confirmation must validation-error, got {result:?}");
    };

    ShredCopy {
        warning_screen_id,
        warning_title,
        continue_action,
        cancel_action,
        first_consequence,
        irreversible_detail,
        confirm_title,
        confirmation_label,
        wipe_action,
        empty_confirmation_error: message,
    }
}

// @scenario: security :: emergency wipe renders in the active locale
// @internal
#[test]
fn shred_wizard_renders_the_active_locale() {
    load_german();
    let de = walk_shred(Locale::German);
    let en = walk_shred(Locale::English);

    // Screen ids are identifiers, not copy — they must NOT translate.
    assert_eq!(de.warning_screen_id, "shred_warning");
    assert_eq!(de.warning_screen_id, en.warning_screen_id);

    assert_translated("warning title", &de.warning_title, &en.warning_title);
    assert_translated("continue action", &de.continue_action, &en.continue_action);
    assert_translated("cancel action", &de.cancel_action, &en.cancel_action);
    assert_translated(
        "first consequence",
        &de.first_consequence,
        &en.first_consequence,
    );
    assert_translated(
        "irreversible detail",
        &de.irreversible_detail,
        &en.irreversible_detail,
    );
    assert_translated("confirm title", &de.confirm_title, &en.confirm_title);
    assert_translated(
        "confirmation input label",
        &de.confirmation_label,
        &en.confirmation_label,
    );
    assert_translated("wipe action", &de.wipe_action, &en.wipe_action);
    assert_translated(
        "empty-confirmation validation",
        &de.empty_confirmation_error,
        &en.empty_confirmation_error,
    );
}

// English stays exactly as it was before the threading (regression pin).
// English is the source language and ships in this repo's bundled locale,
// so pinning it here couples nothing external.
// @internal
#[test]
fn shred_wizard_english_copy_unchanged() {
    let mut engine = EmergencyShredEngine::new(Locale::English);

    let warning = engine.current_screen();
    assert_eq!(warning.title, "Emergency Data Wipe");
    assert_eq!(action_label(&warning, "continue"), "I Understand");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let confirm = engine.current_screen();
    assert_eq!(confirm.title, "Confirm Wipe");
    assert_eq!(action_label(&confirm, "wipe"), "Wipe All Data");

    // The typed token itself stays the literal DELETE in every locale —
    // the gate checks the token, the label explains it.
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "DELETE".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "correct token advances to wiping, got {result:?}"
    );
    assert_eq!(engine.current_screen().screen_id, "shred_wiping");
}
