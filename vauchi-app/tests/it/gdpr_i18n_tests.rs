// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S3b (`2026-07-03-core-screens-bypass-i18n`, design D3.2): the GDPR
//! privacy screens — including the identity-deletion review, the
//! grace-elapsed delete-now confirm, and the panic-shred confirm — render
//! in the user's locale. Exact German assertions per CC-03; keys in the
//! `privacy.*` / `shred.panic_*` families (locales!81).

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    Component, DeletionSummary, GdprEngine, ScreenModel, UserAction, WorkflowEngine,
};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn action_label(screen: &ScreenModel, id: &str) -> String {
    screen
        .contextual_actions
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("action {id} present"))
        .label
        .clone()
}

fn german_engine() -> GdprEngine {
    GdprEngine::new(None, "Active".into(), Locale::German).with_deletion_summary(DeletionSummary {
        contact_count: 3,
        has_backup: false,
        device_count: 2,
    })
}

// @scenario: security :: identity deletion review renders in the active locale
// @internal
#[test]
fn gdpr_overview_and_delete_review_render_german() {
    load_german();
    let mut engine = german_engine();

    let overview = engine.current_screen();
    assert_eq!(overview.screen_id, "privacy_settings");
    assert_eq!(overview.title, "Datenschutz & Daten");
    assert_eq!(action_label(&overview, "delete"), "Identität löschen");
    assert_eq!(action_label(&overview, "export"), "Meine Daten exportieren");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    let review = engine.current_screen();
    assert_eq!(review.title, "Identität löschen");
    assert_eq!(
        review.subtitle.as_deref(),
        Some("Überprüfen Sie, was gelöscht wird")
    );
    let Component::InfoPanel { title, items, .. } = &review.components[0] else {
        panic!("review screen leads with the deletion InfoPanel");
    };
    assert_eq!(title, "Folgendes wird gelöscht");
    assert_eq!(items[0].title, "Ihre Identität");
    // {count} placeholders resolve through get_string_with_args.
    assert_eq!(items[1].title, "3 Kontakt(e)");
    assert!(
        items.iter().any(|i| i.title == "1 verknüpfte(s) Gerät(e)"),
        "linked-device count item localized; items: {:?}",
        items.iter().map(|i| &i.title).collect::<Vec<_>>()
    );
    let irreversible = items.last().expect("irreversible warning item");
    assert_eq!(
        irreversible.title,
        "Dies kann nicht rückgängig gemacht werden"
    );
    assert_eq!(action_label(&review, "cancel"), "Abbrechen");
}

// @scenario: security :: panic shred confirm renders in the active locale
// @internal
#[test]
fn gdpr_panic_shred_confirm_renders_german() {
    load_german();
    let mut engine = german_engine();

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "panic_shred".into(),
    });
    let confirm = engine.current_screen();
    assert_eq!(confirm.title, "Panik-Löschung");
    assert_eq!(confirm.subtitle.as_deref(), Some("Notfall-Löschung"));
    assert_eq!(action_label(&confirm, "confirm_shred"), "Alles vernichten");
    assert_eq!(action_label(&confirm, "cancel"), "Abbrechen");
}

// English stays the canonical key values (two builder literals converge:
// Export My Data + the consent subtitle) — pinned so future edits go
// through the locale files, not the builder.
// @internal
#[test]
fn gdpr_english_copy_matches_canonical_keys() {
    let mut engine = GdprEngine::new(None, "Active".into(), Locale::English);

    let overview = engine.current_screen();
    assert_eq!(overview.title, "Privacy & Data");
    assert_eq!(action_label(&overview, "export"), "Export My Data");

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "consent_actions".into(),
        item_id: "manage_consent".into(),
    });
    let consent = engine.current_screen();
    assert_eq!(consent.title, "Manage Consent");
    assert_eq!(
        consent.subtitle.as_deref(),
        Some("Review and update data consent")
    );
}
