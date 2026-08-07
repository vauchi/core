// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the contact-list empty
//! state renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{ContactListEngine, WorkflowEngine};

/// `(title, add-contact action label)` for the empty state.
fn empty_state_copy(locale: Locale) -> (String, String) {
    let engine = ContactListEngine::new(vec![]).with_locale(locale);
    let screen = engine.current_screen();
    (screen.title.clone(), action_label(&screen, "add_contact"))
}

// @scenario: contacts :: empty contact list renders in the active locale
// @internal
#[test]
fn contact_list_empty_state_renders_the_active_locale() {
    load_german();
    let (de_title, de_add) = empty_state_copy(Locale::German);
    let (en_title, en_add) = empty_state_copy(Locale::English);

    assert_translated("contact-list title", &de_title, &en_title);
    assert_translated("add-contact action", &de_add, &en_add);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn contact_list_english_copy_unchanged() {
    let engine = ContactListEngine::new(vec![]);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Contacts");
    assert!(
        screen
            .contextual_actions
            .iter()
            .any(|a| a.id == "add_contact" && a.label == "Add Contact"),
        "English add-contact label unchanged; actions: {:?}",
        screen.contextual_actions
    );
}
