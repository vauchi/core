// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-13 (`2026-07-03-core-screens-bypass-i18n`): batch-2 of the
//! small remaining files render in the user's locale. Keys in
//! `contact_merge.*`/`contact_limit.*`/`my_info_entry_detail.*`/
//! `delivery_status.*`/`group_detail.*`/`contact_edit.*`
//! (locales!111). Exact German assertions per CC-03.
//! One test per engine — the full a11y/copy detail is covered by each
//! engine's own inline unit tests.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    ContactEditEngine, ContactLimitEngine, ContactMergeEngine, DeliveryStatusEngine,
    EditableContact, GroupDetailEngine, MergePreview, WorkflowEngine,
};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: contact-merge :: screen renders in the active locale
// @internal
#[test]
fn contact_merge_renders_german() {
    load_german();
    let preview = MergePreview {
        primary_name: "Alice".into(),
        primary_fields: vec![],
        secondary_name: "Al".into(),
        secondary_fields: vec![],
    };
    let engine = ContactMergeEngine::new(preview).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Kontakte zusammenführen");
}

// @scenario: contact-limit :: screen renders in the active locale
// @internal
#[test]
fn contact_limit_renders_german() {
    load_german();
    let engine = ContactLimitEngine::new(5, 100).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Kontaktlimit");
}

// @scenario: delivery-status :: screen renders in the active locale
// @internal
#[test]
fn delivery_status_renders_german() {
    load_german();
    let engine = DeliveryStatusEngine::new(vec![]).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Zustellungsstatus");
}

// @scenario: group-detail :: screen renders in the active locale
// @internal
#[test]
fn group_detail_renders_german() {
    load_german();
    let engine =
        GroupDetailEngine::new("g1".into(), "Work".into(), vec![]).with_locale(Locale::German);
    let screen = engine.current_screen();
    let has_group_info = screen.components.iter().any(|c| {
        matches!(c, vauchi_app::ui::Component::InfoPanel { title, .. } if title == "Gruppeninfo")
    });
    assert!(has_group_info, "InfoPanel title must render in German");
}

// @scenario: contact-edit :: screen renders in the active locale
// @internal
#[test]
fn contact_edit_renders_german() {
    load_german();
    let contact = EditableContact {
        display_name: "Alice".into(),
        fields: vec![],
    };
    let engine = ContactEditEngine::new(contact, vec![]).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Kontakt bearbeiten");
}

// English stays exactly as before (regression pin) — one representative check.
// @internal
#[test]
fn batch2_english_copy_unchanged() {
    let preview = MergePreview {
        primary_name: "Alice".into(),
        primary_fields: vec![],
        secondary_name: "Al".into(),
        secondary_fields: vec![],
    };
    assert_eq!(
        ContactMergeEngine::new(preview).current_screen().title,
        "Merge Contacts"
    );
    assert_eq!(
        ContactLimitEngine::new(5, 100).current_screen().title,
        "Contact Limit"
    );
    assert_eq!(
        DeliveryStatusEngine::new(vec![]).current_screen().title,
        "Delivery Status"
    );
    let contact = EditableContact {
        display_name: "Alice".into(),
        fields: vec![],
    };
    assert_eq!(
        ContactEditEngine::new(contact, vec![])
            .current_screen()
            .title,
        "Edit Contact"
    );
}
