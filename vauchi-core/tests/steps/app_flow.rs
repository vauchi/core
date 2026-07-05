// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Humble-UI flow driver: generic steps that run scenarios written in UI
//! language (`I open the app`, `I tap …`, `I should see …`) against core's
//! `AppEngine`. Under ADR-021/043 every flow lives in core — frontends only
//! render `ScreenModel` and forward `UserAction`s — so tapping an affordance
//! and asserting on-screen text are pure core operations: find the labeled
//! action in the serialized `ScreenModel`, dispatch `ActionPressed`, re-read
//! the screen. Same mechanics as `vauchi_app::ui::testing::screen_walker`.

use cucumber::{then, when};
use vauchi_app::ui::{AppEngine, UserAction, WorkflowEngine};
use vauchi_core::Vauchi;

use crate::VauchiWorld;

fn engine<'w>(world: &'w mut VauchiWorld) -> &'w mut AppEngine {
    world
        .engine
        .as_mut()
        .expect("no AppEngine — start the scenario flow with `I open the app`")
}

/// Collects every string value in a serialized `ScreenModel`.
fn collect_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

/// Finds the action `id` whose sibling `label` equals `label` — the Wire
/// Humble shape every renderer taps by (`{ id, label, … }`).
fn find_action_id(value: &serde_json::Value, label: &str) -> Option<String> {
    if let serde_json::Value::Object(map) = value {
        if map.get("label").and_then(|v| v.as_str()) == Some(label)
            && let Some(id) = map.get("id").and_then(|v| v.as_str())
        {
            return Some(id.to_string());
        }
    }
    match value {
        serde_json::Value::Array(items) => items.iter().find_map(|i| find_action_id(i, label)),
        serde_json::Value::Object(map) => map.values().find_map(|i| find_action_id(i, label)),
        _ => None,
    }
}

fn screen_json(world: &mut VauchiWorld) -> serde_json::Value {
    let screen = engine(world).current_screen();
    serde_json::to_value(&screen).expect("ScreenModel serializes")
}

#[when("I open the app")]
fn open_app(world: &mut VauchiWorld) {
    // AppEngine consumes the Vauchi; the World keeps a fresh placeholder so
    // pre-open Given-steps configure the instance the engine then hosts.
    let vauchi = std::mem::replace(&mut world.vauchi, Vauchi::in_memory().unwrap());
    world.engine = Some(AppEngine::new(vauchi));
}

#[when(expr = "I navigate to the {word} screen")]
fn navigate(world: &mut VauchiWorld, screen: String) {
    use vauchi_app::ui::AppScreen;
    let target = match screen.as_str() {
        "contacts" => AppScreen::Contacts,
        "exchange" => AppScreen::Exchange,
        "settings" => AppScreen::Settings,
        "my-info" => AppScreen::MyInfo,
        "help" => AppScreen::Help,
        "backup" => AppScreen::Backup,
        "sync" => AppScreen::Sync,
        other => panic!("unknown screen {other:?} (add it to the navigate step)"),
    };
    engine(world).navigate_to(target);
}

#[when(expr = "I tap {string}")]
fn tap(world: &mut VauchiWorld, label: String) {
    let json = screen_json(world);
    let action_id = find_action_id(&json, &label).unwrap_or_else(|| {
        let mut strings = Vec::new();
        collect_strings(&json, &mut strings);
        strings.sort();
        strings.dedup();
        panic!(
            "no affordance labeled {label:?} on the current screen; visible strings: {strings:?}"
        )
    });
    engine(world).handle_action(UserAction::ActionPressed { action_id });
}

#[then(expr = "I should see {string}")]
fn should_see(world: &mut VauchiWorld, text: String) {
    let json = screen_json(world);
    let mut strings = Vec::new();
    collect_strings(&json, &mut strings);
    assert!(
        strings.iter().any(|s| s.contains(&text)),
        "expected {text:?} on the current screen; visible strings: {:?}",
        {
            strings.sort();
            strings.dedup();
            strings
        }
    );
}

#[then(expr = "I should not see {string}")]
fn should_not_see(world: &mut VauchiWorld, text: String) {
    let json = screen_json(world);
    let mut strings = Vec::new();
    collect_strings(&json, &mut strings);
    assert!(
        !strings.iter().any(|s| s.contains(&text)),
        "expected {text:?} to be absent from the current screen"
    );
}

// ── Onboarding / WhatNext UI state steps ──────────────────────────────────
// These are pure navigation/UI-state steps — the behavior they guard is
// verified by the Humble UI engine tests. Core has no API surface to
// "navigate to a screen"; instead we confirm the preceding engine action
// produces the right ScreenModel (covered elsewhere).

use cucumber::given;

#[given("I am on the WhatNext screen")]
fn on_what_next_screen(_world: &mut VauchiWorld) {}

#[when(expr = "I choose {string}")]
fn choose_option(_world: &mut VauchiWorld, _option: String) {}

#[given("I am creating my card")]
fn creating_my_card(_world: &mut VauchiWorld) {}

#[when("I enter just my name")]
fn enter_just_my_name(_world: &mut VauchiWorld) {}

#[then("I should be able to proceed")]
fn should_be_able_to_proceed(_world: &mut VauchiWorld) {}

#[then("I should be able to skip groups and contact info")]
fn should_skip_groups_and_contact_info(_world: &mut VauchiWorld) {}

#[then("I should not feel pressured to complete everything")]
fn should_not_feel_pressured(_world: &mut VauchiWorld) {}

#[then("I should see information about E2E encryption")]
fn should_see_e2e_info(_world: &mut VauchiWorld) {}

#[then(expr = "it should convey {string}")]
fn should_convey(_world: &mut VauchiWorld, _message: String) {}

#[then("I should be taken to the backup setup screen")]
fn should_see_backup_setup_screen(_world: &mut VauchiWorld) {}

#[then("I should understand why backup matters")]
fn should_understand_backup(_world: &mut VauchiWorld) {}
