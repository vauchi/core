// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The owner sets, changes or removes the recovery passphrase on the
//! social recovery screen (vauchi/private#796, D16): typed twice, held to
//! the backup password's strength, and the screen says whether one is set.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

const STRONG: &str = "the owl flies at noon";

fn owner() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Recovery);
    engine
}

fn screen_of(result: ActionResult) -> ScreenModel {
    match result {
        ActionResult::UpdateScreen(screen) | ActionResult::NavigateTo(screen) => screen,
        other => panic!("expected a screen, got {other:?}"),
    }
}

/// The passphrase row on the first step, as (label, detail).
fn passphrase_row(screen: &ScreenModel) -> Option<(String, Option<String>)> {
    screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::ButtonList { id, items } if id == "recovery_actions" => items
                .iter()
                .find(|item| item.id == "recovery_passphrase")
                .map(|item| (item.label.clone(), item.detail.clone())),
            _ => None,
        })
}

fn open(engine: &mut AppEngine) -> ScreenModel {
    screen_of(engine.handle_action(UserAction::ListItemSelected {
        component_id: "recovery_actions".into(),
        item_id: "recovery_passphrase".into(),
    }))
}

fn type_into(engine: &mut AppEngine, component_id: &str, value: &str) {
    let screen = screen_of(engine.handle_action(UserAction::TextChanged {
        component_id: component_id.into(),
        value: value.into(),
    }));
    let shown = screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::TextInput { id, value, .. } if id == component_id => Some(value.clone()),
            _ => None,
        });
    assert_eq!(shown.as_deref(), Some(value));
}

fn press(engine: &mut AppEngine, action_id: &str) -> ScreenModel {
    screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    }))
}

fn has(screen: &ScreenModel, wanted: &str) -> bool {
    screen.components.iter().any(|component| match component {
        Component::Text { id, .. } | Component::TextInput { id, .. } => id == wanted,
        _ => false,
    })
}

fn actions(screen: &ScreenModel) -> Vec<String> {
    screen
        .contextual_actions
        .iter()
        .map(|action| action.id.clone())
        .collect()
}

fn is_set(engine: &AppEngine) -> bool {
    engine.vauchi().has_recovery_passphrase().unwrap()
}

fn save(engine: &mut AppEngine, first: &str, second: &str) -> ScreenModel {
    type_into(engine, "recovery_passphrase_new", first);
    type_into(engine, "recovery_passphrase_confirm", second);
    press(engine, "save_recovery_passphrase")
}

// @internal
#[test]
fn the_recovery_screen_says_whether_a_passphrase_is_set() {
    let mut engine = owner();
    let before = passphrase_row(&engine.current_screen()).unwrap();
    open(&mut engine);

    let after = passphrase_row(&save(&mut engine, STRONG, STRONG)).unwrap();

    assert_eq!(before.0, after.0);
    assert_ne!(before.1, after.1, "the detail changes once one is set");
    assert!(is_set(&engine));
}

// @internal
#[test]
fn the_step_asks_twice_and_offers_no_removal_while_none_is_set() {
    let mut engine = owner();

    let screen = open(&mut engine);

    assert!(has(&screen, "recovery_passphrase_new"));
    assert!(has(&screen, "recovery_passphrase_confirm"));
    assert!(has(&screen, "recovery_passphrase_explanation"));
    assert_eq!(
        actions(&screen),
        ["save_recovery_passphrase", "cancel_recovery_passphrase"]
    );
}

// @internal
#[test]
fn two_different_entries_are_refused_and_nothing_is_set() {
    let mut engine = owner();
    open(&mut engine);

    let screen = save(&mut engine, STRONG, "the owl flies at dusk");

    assert!(has(&screen, "recovery_passphrase_error"));
    assert!(!is_set(&engine));
}

// @internal
#[test]
fn a_weak_passphrase_is_refused_with_why() {
    let mut engine = owner();
    open(&mut engine);

    let screen = save(&mut engine, "password", "password");

    assert!(has(&screen, "recovery_passphrase_error"));
    assert!(!is_set(&engine));
}

// @internal
#[test]
fn a_set_passphrase_can_be_removed() {
    let mut engine = owner();
    open(&mut engine);
    save(&mut engine, STRONG, STRONG);
    let screen = open(&mut engine);
    assert!(actions(&screen).contains(&"remove_recovery_passphrase".into()));

    let screen = press(&mut engine, "remove_recovery_passphrase");

    assert!(!is_set(&engine));
    assert!(passphrase_row(&screen).is_some(), "back on the first step");
}

// @internal
#[test]
fn cancelling_changes_nothing_and_forgets_what_was_typed() {
    let mut engine = owner();
    open(&mut engine);
    type_into(&mut engine, "recovery_passphrase_new", STRONG);

    press(&mut engine, "cancel_recovery_passphrase");
    let screen = open(&mut engine);

    assert!(!is_set(&engine));
    let typed = screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::TextInput { id, value, .. } if id == "recovery_passphrase_new" => {
                Some(value.clone())
            }
            _ => None,
        });
    assert_eq!(typed.as_deref(), Some(""));
}
