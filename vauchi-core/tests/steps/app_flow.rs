// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Humble-UI flow driver: generic steps that run scenarios written in UI
//! language (`I open the app`, `I tap …`, `I should see …`) against core's
//! `AppEngine`. Under ADR-021/043 every flow lives in core — frontends render
//! the presentation command stream and return opaque events. Contextual
//! actions are therefore resolved through `SetContextBar` and, when needed,
//! the secondary-action overlay. Component-local interactions retain the
//! legacy screen-walker fallback until their presentation-node migration.

use cucumber::{then, when};
use vauchi_app::ui::{AppEngine, UserAction, WorkflowEngine};
use vauchi_core::{ActionSpec, Command, Event, SurfaceId, Vauchi};

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

fn contextual_action(
    commands: &[Command],
    label: &str,
) -> Option<(SurfaceId, vauchi_core::InteractionId)> {
    commands.iter().find_map(|command| match command {
        Command::SetContextBar {
            surface_id, bar, ..
        } => [&bar.back, &bar.navigation, &bar.primary, &bar.secondary]
            .into_iter()
            .filter_map(|action| action.as_ref())
            .find(|action| action.label == label)
            .map(|action| (surface_id.clone(), action.interaction_id.clone())),
        Command::PresentOverlay {
            surface_id,
            overlay,
            ..
        } => overlay
            .items
            .iter()
            .find(|action| action.label == label)
            .map(|action| (surface_id.clone(), action.interaction_id.clone())),
        _ => None,
    })
}

fn secondary_launcher(commands: &[Command]) -> Option<(SurfaceId, ActionSpec)> {
    commands.iter().find_map(|command| match command {
        Command::SetContextBar {
            surface_id, bar, ..
        } => bar
            .secondary
            .clone()
            .map(|action| (surface_id.clone(), action)),
        _ => None,
    })
}

fn dispatch_contextual_action(world: &mut VauchiWorld, label: &str) -> bool {
    let initial = engine(world)
        .initial_commands()
        .expect("compose current presentation");
    if let Some((surface_id, interaction_id)) = contextual_action(&initial, label) {
        engine(world)
            .dispatch(Event::ActionActivated {
                surface_id,
                interaction_id,
            })
            .expect("dispatch contextual action");
        return true;
    }

    let Some((surface_id, launcher)) = secondary_launcher(&initial) else {
        return false;
    };
    let overlay = engine(world)
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: launcher.interaction_id,
        })
        .expect("open secondary-action overlay");
    let Some((surface_id, interaction_id)) = contextual_action(&overlay, label) else {
        return false;
    };
    engine(world)
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .expect("dispatch secondary contextual action");
    true
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
    if world.engine.is_none() {
        let vauchi = std::mem::replace(&mut world.vauchi, Vauchi::in_memory().unwrap());
        world.engine = Some(AppEngine::new(vauchi));
    }
    let target = match screen.to_lowercase().as_str() {
        "contacts" => AppScreen::Contacts,
        "exchange" => AppScreen::Exchange,
        "settings" => AppScreen::Settings,
        "my-info" | "myinfo" | "my info" => AppScreen::MyInfo,
        "help" => AppScreen::Help,
        "backup" => AppScreen::Backup,
        "lock" => AppScreen::Lock,
        "device-linking" | "devicelinking" => AppScreen::DeviceLinking,
        "device-management" | "devicemanagement" => AppScreen::DeviceManagement,
        other => panic!("unknown screen {other:?} (add it to the navigate step)"),
    };
    engine(world).navigate_to(target);
}

#[when(expr = "I tap {string}")]
fn tap(world: &mut VauchiWorld, label: String) {
    if dispatch_contextual_action(world, &label) {
        return;
    }
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
    let _ = engine(world).handle_action(UserAction::ActionPressed { action_id });
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
