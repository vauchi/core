// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S3 (`2026-07-03-core-screens-bypass-i18n`, design D3.2): the
//! emergency-wipe screen renders in the user's locale, threaded from the
//! engine entry point — the first of the destructive/security confirmation
//! screens to leave hardcoded English behind. Keys live in the
//! `shred.wipe.*` family (locales!80).
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{ActionResult, Component, EmergencyShredEngine, UserAction, WorkflowEngine};

/// Copy a shell would show on the wipe screen, up to its validation error.
struct ShredCopy {
    screen_id: String,
    title: String,
    subtitle: String,
    first_consequence: String,
    irreversible_detail: String,
    confirmation_label: String,
    confirm_warning: String,
    shred_button: String,
    cancel_button: String,
    empty_confirmation_error: String,
}

fn walk_shred(locale: Locale) -> ShredCopy {
    let mut engine = EmergencyShredEngine::new(locale);

    let screen = engine.current_screen();
    let Component::InfoPanel { items, .. } = &screen.components[0] else {
        panic!("the wipe screen leads with the consequences InfoPanel");
    };
    let Component::TextInput { label, .. } = &screen.components[1] else {
        panic!("the typed-confirmation input follows the consequences");
    };
    let Component::InlineConfirm {
        warning,
        confirm_text,
        cancel_text,
        ..
    } = &screen.components[2]
    else {
        panic!("the Shred Everything / Cancel pair closes the screen");
    };

    // The wrong-text validation message is localized too.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_shred".into(),
    });
    let ActionResult::ValidationError { message, .. } = result else {
        panic!("empty confirmation must validation-error, got {result:?}");
    };

    ShredCopy {
        screen_id: screen.screen_id.clone(),
        title: screen.title.clone(),
        subtitle: screen.subtitle.clone().unwrap_or_default(),
        first_consequence: items[0].title.clone(),
        irreversible_detail: items[2].detail.clone(),
        confirmation_label: label.clone(),
        confirm_warning: warning.clone(),
        shred_button: confirm_text.clone(),
        cancel_button: cancel_text.clone(),
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
    assert_eq!(de.screen_id, "shred_warning");
    assert_eq!(de.screen_id, en.screen_id);

    assert_translated("title", &de.title, &en.title);
    assert_translated("subtitle", &de.subtitle, &en.subtitle);
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
    assert_translated(
        "confirmation input label",
        &de.confirmation_label,
        &en.confirmation_label,
    );
    assert_translated("confirm warning", &de.confirm_warning, &en.confirm_warning);
    assert_translated("shred button", &de.shred_button, &en.shred_button);
    assert_translated("cancel button", &de.cancel_button, &en.cancel_button);
    assert_translated(
        "empty-confirmation validation",
        &de.empty_confirmation_error,
        &en.empty_confirmation_error,
    );
}

// The typed token itself stays the literal WIPE in every locale — the
// gate checks the token, the label explains it.
// @internal
#[test]
fn the_confirmation_word_is_not_translated() {
    load_german();
    let mut engine = EmergencyShredEngine::new(Locale::German);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "WIPE".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_shred".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}
