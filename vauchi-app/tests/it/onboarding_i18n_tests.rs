// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S4a (`2026-07-03-core-screens-bypass-i18n`, user decision 2026-07-04:
//! the flat key generation wins): the onboarding wizard renders in the
//! user's locale via the live-copy `onboarding.*` flat keys (locales!83).
//! Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    ActionResult, Component, OnboardingEngine, ScreenModel, UserAction, WorkflowEngine,
};

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

// @scenario: onboarding :: first launch renders in the active locale
// @internal
#[test]
fn onboarding_welcome_and_name_render_german() {
    load_german();
    let mut engine = OnboardingEngine::new().with_locale(Locale::German);

    let welcome = engine.current_screen();
    assert_eq!(welcome.title, "Willkommen bei Vauchi");
    assert_eq!(
        welcome.subtitle.as_deref(),
        Some("Datenschutzfreundliche Kontaktkarten.")
    );
    assert_eq!(
        action_label(&welcome, "create_new"),
        "Neue Identität erstellen"
    );
    let Component::InfoPanel { items, .. } = &welcome.components[0] else {
        panic!("welcome leads with the value-props InfoPanel");
    };
    assert_eq!(items[0].title, "Privat von Grund auf");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let name = engine.current_screen();
    assert_eq!(name.title, "Wie heißen Sie?");

    // Empty name → localized validation.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::ValidationError { message, .. } = result else {
        panic!("empty name must validation-error, got {result:?}");
    };
    assert_eq!(message, "Bitte geben Sie Ihren Namen ein.");
}

// English stays as it was (with the one deliberate convergence:
// the Skip buttons now read the canonical onboarding.skip value).
// @internal
#[test]
fn onboarding_english_copy_unchanged() {
    let mut engine = OnboardingEngine::new();

    let welcome = engine.current_screen();
    assert_eq!(welcome.title, "Welcome to Vauchi");
    assert_eq!(action_label(&welcome, "create_new"), "Create new identity");
    assert_eq!(
        action_label(&welcome, "have_identity"),
        "I already have an identity"
    );

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    assert_eq!(engine.current_screen().title, "What's your name?");
}
