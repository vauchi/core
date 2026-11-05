// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value;
use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, Component, WorkflowEngine};
use vauchi_core::{Command, Event, InputValue, Vauchi};

fn find_labeled(value: &Value, label: &str, field: &str) -> Option<Value> {
    match value {
        Value::Object(map) => {
            if map.get("label").and_then(Value::as_str) == Some(label)
                && let Some(id) = map.get(field)
            {
                return Some(id.clone());
            }
            map.values().find_map(|v| find_labeled(v, label, field))
        }
        Value::Array(items) => items.iter().find_map(|v| find_labeled(v, label, field)),
        _ => None,
    }
}

fn activate(app: &mut AppEngine, commands: &[Command], key: &str) -> Vec<Command> {
    let surface_id = commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.clone()),
            _ => None,
        })
        .unwrap();
    let id = find_labeled(
        &serde_json::to_value(commands).unwrap(),
        &get_string(Locale::ENGLISH, key),
        "interaction_id",
    )
    .unwrap();
    app.dispatch(Event::ActionActivated {
        surface_id,
        interaction_id: serde_json::from_value(id).unwrap(),
    })
    .unwrap()
}

fn enter_key(app: &mut AppEngine, commands: &[Command], value: &str) -> Vec<Command> {
    let surface_id = commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.clone()),
            _ => None,
        })
        .unwrap();
    let id = find_labeled(
        &serde_json::to_value(commands).unwrap(),
        &get_string(Locale::ENGLISH, "recovery.old_public_key"),
        "binding_id",
    )
    .unwrap();
    app.dispatch(Event::ValueChanged {
        surface_id,
        binding_id: serde_json::from_value(id).unwrap(),
        value: InputValue::Text(value.into()),
    })
    .unwrap()
}

// @internal — P32: the canonical UI path must not mint a replacement identity.
#[test]
fn recovery_dispatch_starts_an_identity_free_claim() {
    let old_key = "01".repeat(32);
    let mut app = AppEngine::new(Vauchi::in_memory().unwrap());
    app.set_initial_screen(AppScreen::Recovery);
    let commands = app.initial_commands().unwrap();
    let commands = activate(&mut app, &commands, "recovery.start_button");
    let commands = enter_key(&mut app, &commands, &old_key);
    let commands = activate(&mut app, &commands, "recovery.create_claim_button");
    assert!(!app.vauchi().has_identity());
    let displayed = app
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::Text { id, content, .. } if id == "claim_data" => Some(content),
            _ => None,
        })
        .expect("the identity-free recovery claim is displayed");
    let claim = app.vauchi().parse_recovery_claim_b64(&displayed).unwrap();
    assert_eq!(claim.old_pk().as_bytes(), &[1; 32]);
    let persisted = app.vauchi().begin_recovery(&old_key).unwrap();
    let persisted = app.vauchi().parse_recovery_claim_b64(&persisted).unwrap();
    assert_eq!(claim.new_pk(), persisted.new_pk());
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::ReplaceSurface { .. }))
    );
}
