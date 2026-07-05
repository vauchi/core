// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6b-2 (`2026-07-03-core-screens-bypass-i18n`): the contact-list
//! screen (title, empty state, add-contact/all-contacts/exchange-now
//! buttons, also-search toggle) renders in the user's locale. Keys in
//! `contacts.*` (locales!91). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{ContactListEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: contacts :: empty contact list renders in the active locale
// @internal
#[test]
fn contact_list_empty_state_renders_german() {
    load_german();
    let engine = ContactListEngine::new(vec![]).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Kontakte");
    assert!(
        screen
            .actions
            .iter()
            .any(|a| a.id == "add_contact" && a.label == "Kontakt hinzufügen"),
        "add-contact action localized; actions: {:?}",
        screen.actions
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn contact_list_english_copy_unchanged() {
    let engine = ContactListEngine::new(vec![]);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Contacts");
    assert!(
        screen
            .actions
            .iter()
            .any(|a| a.id == "add_contact" && a.label == "Add Contact"),
        "English add-contact label unchanged; actions: {:?}",
        screen.actions
    );
}
