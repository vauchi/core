// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the onboarding welcome
//! and name screens render in the user's locale.
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german, row_label};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, AppEngine, Component, OnboardingEngine, RenderContext, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

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
    let items = welcome
        .components
        .iter()
        .find_map(|c| match c {
            Component::InfoPanel { items, .. } => Some(items),
            _ => None,
        })
        .expect("welcome carries the value-props InfoPanel");
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
    let de = walk_onboarding(Locale::GERMAN);
    let en = walk_onboarding(Locale::ENGLISH);

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
    assert_eq!(welcome.title, "Welcome");
    assert_eq!(
        welcome.subtitle.as_deref(),
        Some("Your contact card, shared in person and kept up to date.")
    );
    assert_eq!(action_label(&welcome, "create_new"), "Create new identity");
    assert_eq!(row_label(&welcome, "link_device"), "Link this device");
    assert_eq!(row_label(&welcome, "load_backup"), "Restore from backup");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    assert_eq!(engine.current_screen().title, "What's your name?");
}

fn suggested_group_labels(locale: Locale) -> Vec<(String, String)> {
    let mut engine = OnboardingEngine::new().with_locale(locale);
    for (action_id, name) in [("create_new", None), ("continue", Some("Alice"))] {
        if let Some(name) = name {
            let _ = engine.handle_action(UserAction::TextChanged {
                component_id: "display_name".into(),
                value: name.into(),
            });
        }
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: action_id.into(),
        });
    }
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == "groups" => Some(
                items
                    .iter()
                    .map(|i| (i.id.clone(), i.label.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .expect("groups step carries the suggested-groups toggle list")
}

/// Device run on a fr-CH phone showed "Family/Friends/Coworkers" in
/// English on an otherwise French groups step (vauchi/private#721).
// @internal
#[test]
fn suggested_groups_render_in_the_active_locale_with_stable_ids() {
    load_german();
    let de = suggested_group_labels(Locale::GERMAN);
    let en = suggested_group_labels(Locale::ENGLISH);

    assert_eq!(
        de.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        en.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        "toggle ids are the canonical names, so a toggle reaches the same group in any language"
    );
    for ((id, de_label), (_, en_label)) in de.iter().zip(&en) {
        assert_translated(&format!("suggested group {id}"), de_label, en_label);
    }
}

/// A group is the user's own data once created, so the suggestion they
/// picked is saved under the name they read.
// @internal
#[test]
fn suggested_groups_picked_during_onboarding_are_created_in_the_active_locale() {
    load_german();
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    engine.set_render_context(RenderContext {
        locale: Some("de".into()),
        ..RenderContext::default()
    });
    let shown = suggested_group_labels(Locale::GERMAN);
    let mut act = |action| {
        let _ = engine.handle_action(action);
    };
    act(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    act(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    act(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    act(UserAction::ItemToggled {
        component_id: "groups".into(),
        item_id: "Family".into(),
    });
    for action_id in ["continue", "continue", "start_app"] {
        act(UserAction::ActionPressed {
            action_id: action_id.into(),
        });
    }

    let created: Vec<String> = engine
        .vauchi()
        .list_groups()
        .expect("groups readable after onboarding")
        .iter()
        .map(|g| g.name().to_string())
        .collect();
    let family_label = &shown
        .iter()
        .find(|(id, _)| id == "Family")
        .expect("Family is suggested")
        .1;
    assert_eq!(created, vec![family_label.clone()]);
}
