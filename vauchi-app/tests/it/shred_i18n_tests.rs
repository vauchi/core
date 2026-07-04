// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S3 (`2026-07-03-core-screens-bypass-i18n`, design D3.2): the
//! emergency-wipe wizard renders in the user's locale, threaded from the
//! engine entry point — the first of the destructive/security confirmation
//! screens to leave hardcoded English behind. Exact German assertions per
//! CC-03; keys live in the `shred.wipe.*` family (locales!80).

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    ActionResult, Component, EmergencyShredEngine, ScreenModel, UserAction, WorkflowEngine,
};

/// Load the real German locale file, exactly as CI does (the `.clone-locales`
/// template places the checkout as a sibling of core, same relative path
/// build.rs bundles English from).
fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn action_label(screen: &ScreenModel, id: &str) -> String {
    screen
        .actions
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("action {id} present"))
        .label
        .clone()
}

// @scenario: security :: emergency wipe renders in the active locale
// @internal
#[test]
fn shred_wizard_renders_german() {
    load_german();
    let mut engine = EmergencyShredEngine::new(Locale::German);

    let warning = engine.current_screen();
    assert_eq!(warning.screen_id, "shred_warning");
    assert_eq!(warning.title, "Notfall-Datenlöschung");
    assert_eq!(action_label(&warning, "continue"), "Ich verstehe");
    assert_eq!(action_label(&warning, "cancel"), "Abbrechen");
    let Component::InfoPanel { items, .. } = &warning.components[0] else {
        panic!("warning screen leads with the consequences InfoPanel");
    };
    assert_eq!(items[0].title, "Alle Kontakte werden gelöscht");
    assert_eq!(
        items[2].detail,
        "Dieser Vorgang kann nicht rückgängig gemacht werden."
    );

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let confirm = engine.current_screen();
    assert_eq!(confirm.title, "Löschung bestätigen");
    let Component::TextInput { label, .. } = &confirm.components[0] else {
        panic!("confirm screen leads with the typed-confirmation input");
    };
    assert_eq!(label, "Geben Sie DELETE ein, um zu bestätigen");
    assert_eq!(action_label(&confirm, "wipe"), "Alle Daten löschen");

    // The wrong-text validation message is localized too.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });
    let ActionResult::ValidationError { message, .. } = result else {
        panic!("empty confirmation must validation-error, got {result:?}");
    };
    assert_eq!(message, "Geben Sie DELETE ein, um zu bestätigen");
}

// English stays exactly as it was before the threading (regression pin).
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
