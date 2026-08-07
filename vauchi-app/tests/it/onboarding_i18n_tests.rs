// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the onboarding welcome
//! and name screens render in the user's locale.
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{ActionResult, Component, OnboardingEngine, UserAction, WorkflowEngine};

/// Copy a shell would show across welcome, name, and the empty-name error.
struct OnboardingCopy {
    welcome_title: String,
    welcome_subtitle: String,
    create_action: String,
    first_value_prop: String,
    name_title: String,
    empty_name_error: String,
}

fn walk_onboarding(locale: Locale) -> OnboardingCopy {
    let mut engine = OnboardingEngine::new().with_locale(locale);

    let welcome = engine.current_screen();
    let welcome_title = welcome.title.clone();
    let welcome_subtitle = welcome.subtitle.clone().expect("welcome subtitle present");
    let create_action = action_label(&welcome, "create_new");
    let Component::InfoPanel { items, .. } = &welcome.components[0] else {
        panic!("welcome leads with the value-props InfoPanel");
    };
    let first_value_prop = items[0].title.clone();

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let name_title = engine.current_screen().title.clone();

    // Empty name → localized validation.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::ValidationError { message, .. } = result else {
        panic!("empty name must validation-error, got {result:?}");
    };

    OnboardingCopy {
        welcome_title,
        welcome_subtitle,
        create_action,
        first_value_prop,
        name_title,
        empty_name_error: message,
    }
}

// @scenario: onboarding :: first launch renders in the active locale
// @internal
#[test]
fn onboarding_welcome_and_name_render_the_active_locale() {
    load_german();
    let de = walk_onboarding(Locale::German);
    let en = walk_onboarding(Locale::English);

    assert_translated("welcome title", &de.welcome_title, &en.welcome_title);
    assert_translated(
        "welcome subtitle",
        &de.welcome_subtitle,
        &en.welcome_subtitle,
    );
    assert_translated(
        "create-identity action",
        &de.create_action,
        &en.create_action,
    );
    assert_translated(
        "first value prop",
        &de.first_value_prop,
        &en.first_value_prop,
    );
    assert_translated("name-step title", &de.name_title, &en.name_title);
    assert_translated(
        "empty-name validation",
        &de.empty_name_error,
        &en.empty_name_error,
    );
}

// English stays as it was (with the one deliberate convergence: the Skip
// buttons now read the canonical onboarding.skip value). English is the
// source language and ships bundled, so pinning it couples nothing
// external.
// @internal
#[test]
fn onboarding_english_copy_unchanged() {
    let mut engine = OnboardingEngine::new();

    let welcome = engine.current_screen();
    assert_eq!(welcome.title, "Welcome to Vauchi");
    assert_eq!(action_label(&welcome, "create_new"), "Create new identity");
    assert_eq!(
        action_label(&welcome, "link_device"),
        "Link from another device"
    );
    assert_eq!(action_label(&welcome, "load_backup"), "Restore from backup");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    assert_eq!(engine.current_screen().title, "What's your name?");
}
