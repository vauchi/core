// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The relay editor takes a custom relay's OHTTP anchor with its URL
//! (#288, plan 6.1, decision 0.9): without one the app refuses the relay,
//! inline on the anchor input, and Vauchi's own relay needs none.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

const DEFAULT_RELAY: &str = "https://relay.vauchi.app";
const CUSTOM_RELAY: &str = "https://relay.self-hosted.example";
const ANCHOR: [u8; 32] = [0x5a; 32];

fn engine_with(vauchi: Vauchi) -> AppEngine {
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::SettingsAdvanced);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "network".into(),
        item_id: "relay_url".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "form_edit_relay_url");
    engine
}

fn fresh_engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    engine_with(vauchi)
}

fn type_into(engine: &mut AppEngine, id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: id.into(),
        value: value.into(),
    });
}

fn submit(engine: &mut AppEngine) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    })
}

fn input_value(engine: &AppEngine, input_id: &str) -> String {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::TextInput { id, value, .. } if id == input_id => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {input_id} input on the relay editor"))
}

fn inline_validation_error(result: ActionResult, component_id: &str) -> String {
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("expected an updated screen, got {result:?}");
    };
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::TextInput {
                id,
                validation_error: Some(message),
                ..
            } if id == component_id => Some(message.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no validation error on {component_id}"))
}

// @internal
#[test]
fn a_custom_relay_is_saved_with_its_anchor() {
    let mut engine = fresh_engine();
    type_into(&mut engine, "relay_url", CUSTOM_RELAY);
    type_into(&mut engine, "relay_anchor", &hex::encode_upper(ANCHOR));

    let result = submit(&mut engine);

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "a relay with a well-formed anchor saves, got {result:?}"
    );
    let relay = &engine.vauchi().config().relay;
    assert_eq!(relay.server_url, CUSTOM_RELAY);
    assert_eq!(relay.ohttp_trust_anchor(), Some(ANCHOR));
}

// @internal
#[test]
fn a_custom_relay_without_an_anchor_is_refused_inline() {
    let mut engine = fresh_engine();
    type_into(&mut engine, "relay_url", CUSTOM_RELAY);

    let message = inline_validation_error(submit(&mut engine), "relay_anchor");

    assert_eq!(message, "A custom relay needs its OHTTP anchor");
    assert_eq!(engine.vauchi().config().relay.server_url, DEFAULT_RELAY);
}

// @internal
#[test]
fn a_malformed_anchor_is_refused_inline() {
    for malformed in ["5a5a", &"zz".repeat(32), &"5a".repeat(33)] {
        let mut engine = fresh_engine();
        type_into(&mut engine, "relay_url", CUSTOM_RELAY);
        type_into(&mut engine, "relay_anchor", malformed);

        let message = inline_validation_error(submit(&mut engine), "relay_anchor");

        assert_eq!(message, "An anchor is 64 hexadecimal characters");
        assert_eq!(engine.vauchi().config().relay.server_url, DEFAULT_RELAY);
    }
}

// @internal
#[test]
fn vauchis_relay_needs_no_anchor() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    vauchi.set_relay(CUSTOM_RELAY, Some(ANCHOR)).unwrap();
    let mut engine = engine_with(vauchi);
    type_into(&mut engine, "relay_url", DEFAULT_RELAY);
    type_into(&mut engine, "relay_anchor", "");

    let result = submit(&mut engine);

    assert!(matches!(result, ActionResult::NavigateTo(_)), "{result:?}");
    assert_eq!(engine.vauchi().config().relay.server_url, DEFAULT_RELAY);
}

// @internal
#[test]
fn the_editor_shows_the_custom_relays_anchor() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    vauchi.set_relay(CUSTOM_RELAY, Some(ANCHOR)).unwrap();

    let engine = engine_with(vauchi);

    assert_eq!(input_value(&engine, "relay_url"), CUSTOM_RELAY);
    assert_eq!(input_value(&engine, "relay_anchor"), hex::encode(ANCHOR));
}

// @internal
#[test]
fn the_editor_shows_no_anchor_for_vauchis_relay() {
    let engine = fresh_engine();

    assert_eq!(input_value(&engine, "relay_anchor"), "");
}
