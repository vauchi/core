// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value;
use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, Component, WorkflowEngine};
use vauchi_core::{Command, Event, InputValue, SymmetricKey, Vauchi, VauchiConfig};

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

// @internal — DC-01/DC-02: invalid input cannot start a recovery or create an identity.
#[test]
fn recovery_dispatch_refuses_malformed_old_key_without_initializing_identity() {
    let mut app = AppEngine::new(Vauchi::in_memory().unwrap());
    app.set_initial_screen(AppScreen::Recovery);
    let commands = app.initial_commands().unwrap();
    let commands = activate(&mut app, &commands, "recovery.start_button");
    let commands = enter_key(&mut app, &commands, &"zz".repeat(32));
    activate(&mut app, &commands, "recovery.create_claim_button");
    assert!(!app.vauchi().has_identity());
    let screen = app.current_screen();
    assert!(screen.components.iter().any(|component| matches!(component,
        Component::TextInput { id, validation_error: Some(error), .. }
            if id == "old_public_key" && !error.is_empty())));
    assert!(
        !screen.components.iter().any(|component| matches!(component,
        Component::Text { id, .. } if id == "claim_data"))
    );
}

// @internal — the existing-identity claim path remains distinct from identity-free restore.
#[test]
fn recovery_dispatch_preserves_the_existing_identity_claim_path() {
    let mut phone = Vauchi::in_memory().unwrap();
    phone.create_identity("Alice").unwrap();
    let expected = *phone.identity().unwrap().signing_public_key();
    let mut app = AppEngine::new(phone);
    app.set_initial_screen(AppScreen::Recovery);
    let commands = app.initial_commands().unwrap();
    let commands = activate(&mut app, &commands, "recovery.start_button");
    let commands = enter_key(&mut app, &commands, &"01".repeat(32));
    activate(&mut app, &commands, "recovery.create_claim_button");
    let displayed = app
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::Text { id, content, .. } if id == "claim_data" => Some(content),
            _ => None,
        })
        .unwrap();
    let claim = app.vauchi().parse_recovery_claim_b64(&displayed).unwrap();
    assert_eq!(claim.new_pk().as_bytes(), &expected);
    assert_eq!(
        app.vauchi().identity().unwrap().signing_public_key(),
        &expected
    );
}

// @internal — P32: a fresh install reaches recovery using only emitted actions.
#[test]
fn recovery_dispatch_onboarding_reaches_identity_free_claim_creation() {
    let mut app = AppEngine::new(Vauchi::in_memory().unwrap());
    let commands = app.initial_commands().unwrap();
    assert_eq!(app.current_screen().screen_id, "identity_check");
    let commands = activate(&mut app, &commands, "recovery.start_button");
    assert_eq!(app.current_app_screen(), &AppScreen::Recovery);
    assert!(!app.vauchi().has_identity());
    let commands = activate(&mut app, &commands, "recovery.start_button");
    let commands = enter_key(&mut app, &commands, &"01".repeat(32));
    activate(&mut app, &commands, "recovery.create_claim_button");
    let displayed = app
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::Text { id, content, .. } if id == "claim_data" => Some(content),
            _ => None,
        })
        .expect("onboarding leads to a claim without creating an identity");
    assert_eq!(
        app.vauchi()
            .parse_recovery_claim_b64(&displayed)
            .unwrap()
            .old_pk()
            .as_bytes(),
        &[1; 32]
    );
    assert!(!app.vauchi().has_identity());
}

// @internal — P32: reopening an identity-free ceremony keeps its target and temporary key.
#[test]
fn recovery_dispatch_reopens_the_stored_identity_free_claim_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("phone.db");
    let storage_key = SymmetricKey::generate();
    let config = || VauchiConfig::with_storage_path(&path).with_storage_key(storage_key.clone());
    let original = {
        let phone = Vauchi::new(config()).unwrap();
        let claim = phone.begin_recovery(&"01".repeat(32)).unwrap();
        phone.parse_recovery_claim_b64(&claim).unwrap()
    };

    let mut app = AppEngine::new(Vauchi::new(config()).unwrap());
    let commands = app.initial_commands().unwrap();
    assert_eq!(app.current_app_screen(), &AppScreen::Recovery);
    let displayed = app
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::Text { id, content, .. } if id == "claim_data" => Some(content),
            _ => None,
        })
        .expect("restart presents the stored recovery claim");
    let resumed = app.vauchi().parse_recovery_claim_b64(&displayed).unwrap();
    assert_eq!(resumed.old_pk(), original.old_pk());
    assert_eq!(resumed.new_pk(), original.new_pk());
    assert!(!app.vauchi().has_identity());
    assert!(commands.iter().any(|command| matches!(command,
        Command::ReplaceSurface { surface } if surface.surface_id.as_str() == "recovery")));
}

// @internal — DC-02: unreadable recovery stays recoverable through supplied retry actions.
#[cfg(feature = "testing")]
#[test]
fn recovery_dispatch_storage_error_is_retryable_without_entering_onboarding() {
    let phone = Vauchi::in_memory().unwrap();
    let original = phone.begin_recovery(&"01".repeat(32)).unwrap();
    let original = phone.parse_recovery_claim_b64(&original).unwrap();
    let sealed: Vec<u8> = phone
        .storage()
        .connection()
        .query_row(
            "SELECT seed_encrypted FROM recovery_mode_key WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    phone
        .storage()
        .connection()
        .execute("UPDATE recovery_mode_key SET seed_encrypted = X'00'", [])
        .unwrap();
    let mut app = AppEngine::new(phone);
    let commands = app.initial_commands().unwrap();
    assert_eq!(app.current_app_screen(), &AppScreen::Recovery);
    assert_eq!(
        app.current_screen().title,
        get_string(Locale::ENGLISH, "recovery.load_error")
    );
    let commands = activate(&mut app, &commands, "action.try_again");
    assert_eq!(
        app.current_screen().title,
        get_string(Locale::ENGLISH, "recovery.load_error")
    );
    assert!(!app.vauchi().has_identity());

    app.vauchi()
        .storage()
        .connection()
        .execute("UPDATE recovery_mode_key SET seed_encrypted = ?1", [sealed])
        .unwrap();
    activate(&mut app, &commands, "action.try_again");
    let displayed = app
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::Text { id, content, .. } if id == "claim_data" => Some(content),
            _ => None,
        })
        .expect("retry reloads the repaired recovery state");
    let resumed = app.vauchi().parse_recovery_claim_b64(&displayed).unwrap();
    assert_eq!(resumed.old_pk(), original.old_pk());
    assert_eq!(resumed.new_pk(), original.new_pk());
    assert!(!app.vauchi().has_identity());
}
