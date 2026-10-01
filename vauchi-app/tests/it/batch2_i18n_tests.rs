// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the second batch of
//! screens renders in the user's locale (locales!111). One test per
//! engine — the full a11y/copy detail is covered by each engine's own
//! inline unit tests.
//!
//! Asserts that each screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ContactEditEngine, ContactMergeEngine, DeliveryStatusEngine, EditableContact,
    GroupDetailEngine, MergePreview, WorkflowEngine,
};

fn merge_preview() -> MergePreview {
    MergePreview {
        primary_name: "Alice".into(),
        primary_fields: vec![],
        secondary_name: "Al".into(),
        secondary_fields: vec![],
    }
}

fn editable_contact() -> EditableContact {
    EditableContact {
        card_name: "Alice".into(),
        display_name: "Alice".into(),
        personal_note: String::new(),
    }
}

fn merge_title(locale: Locale) -> String {
    ContactMergeEngine::new(merge_preview())
        .with_locale(locale)
        .current_screen()
        .title
        .clone()
}

fn delivery_title(locale: Locale) -> String {
    DeliveryStatusEngine::new(vec![])
        .with_locale(locale)
        .current_screen()
        .title
        .clone()
}

/// The group-detail InfoPanel title.
fn group_info_panel_title(locale: Locale) -> String {
    GroupDetailEngine::new("g1".into(), "Work".into(), vec![])
        .with_locale(locale)
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::InfoPanel { title, .. } => Some(title.clone()),
            _ => None,
        })
        .expect("group detail renders an InfoPanel")
}

/// `(screen title, field-list title)` for contact edit.
fn contact_edit_copy(locale: Locale) -> (String, String) {
    let engine = ContactEditEngine::new(editable_contact()).with_locale(locale);
    let screen = engine.current_screen();
    let note_label = screen
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::TextInput { id, label, .. } if id == "personal_note" => {
                Some(label.clone())
            }
            _ => None,
        })
        .expect("contact edit renders the personal note input");
    (screen.title.clone(), note_label)
}

// @scenario: contact-merge :: screen renders in the active locale
// @internal
#[test]
fn contact_merge_renders_the_active_locale() {
    load_german();
    assert_translated(
        "contact-merge title",
        &merge_title(Locale::German),
        &merge_title(Locale::English),
    );
}

// @scenario: delivery-status :: screen renders in the active locale
// @internal
#[test]
fn delivery_status_renders_the_active_locale() {
    load_german();
    assert_translated(
        "delivery-status title",
        &delivery_title(Locale::German),
        &delivery_title(Locale::English),
    );
}

// @scenario: group-detail :: screen renders in the active locale
// @internal
#[test]
fn group_detail_renders_the_active_locale() {
    load_german();
    assert_translated(
        "group-info panel title",
        &group_info_panel_title(Locale::German),
        &group_info_panel_title(Locale::English),
    );
}

// @scenario: contact-edit :: screen renders in the active locale
// @internal
#[test]
fn contact_edit_renders_the_active_locale() {
    load_german();
    let (de_title, de_fields) = contact_edit_copy(Locale::German);
    let (en_title, en_fields) = contact_edit_copy(Locale::English);

    assert_translated("contact-edit title", &de_title, &en_title);
    assert_translated("personal note label", &de_fields, &en_fields);
}

// English stays exactly as before (regression pin) — one representative
// check. English is the source language and ships bundled, so pinning it
// couples nothing external.
// @internal
#[test]
fn batch2_english_copy_unchanged() {
    assert_eq!(merge_title(Locale::English), "Merge Contacts");
    assert_eq!(delivery_title(Locale::English), "Update Delivery");
    assert_eq!(contact_edit_copy(Locale::English).0, "Edit Contact");
}
