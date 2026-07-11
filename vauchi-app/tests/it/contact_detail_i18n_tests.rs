// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-12 (`2026-07-03-core-screens-bypass-i18n`): the contact-detail
//! and contact-not-found screens render in the user's locale. Keys in
//! `contact_detail.*` (locales!110). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{ContactDetailEngine, ContactNotFoundEngine, Field, Item, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

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

// @scenario: contact-detail :: main screen renders in the active locale
// @internal
#[test]
fn contact_detail_renders_german() {
    load_german();
    let engine = ContactDetailEngine::new(sample_contact(), Vec::<Field>::new(), String::new())
        .with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "edit")
            .unwrap()
            .label,
        "Bearbeiten"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn contact_detail_english_copy_unchanged() {
    let engine = ContactDetailEngine::new(sample_contact(), Vec::<Field>::new(), String::new());
    let screen = engine.current_screen();
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "edit")
            .unwrap()
            .label,
        "Edit"
    );
}

// @scenario: contact-detail :: not-found screen renders in the active locale
// @internal
#[test]
fn contact_not_found_renders_german() {
    load_german();
    let engine = ContactNotFoundEngine::new("c1".into()).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Kontakt nicht gefunden");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "back")
            .unwrap()
            .label,
        "Zurück"
    );
}

// @internal
#[test]
fn contact_not_found_english_copy_unchanged() {
    let engine = ContactNotFoundEngine::new("c1".into());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Contact Not Found");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "back")
            .unwrap()
            .label,
        "Back"
    );
}
