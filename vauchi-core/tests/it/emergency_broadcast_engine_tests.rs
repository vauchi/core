// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `EmergencyBroadcastEngine` — the humble engine behind the
//! emergency-broadcast screen. Contact-id parsing and the trusted-contact
//! limit live in the engine (core), not the frontend (ADR-021 / ADR-043).

use vauchi_app::ui::*;
use vauchi_core::types::EmergencyBroadcastConfig;

fn configured() -> EmergencyBroadcastConfig {
    EmergencyBroadcastConfig {
        trusted_contact_ids: vec!["abc".into(), "def".into()],
        message: "help".into(),
        include_location: true,
    }
}

// @internal
#[test]
fn overview_unconfigured_offers_only_configure() {
    let engine = EmergencyBroadcastEngine::new(None);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "emergency_overview");
    let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["configure"], "unconfigured offers only configure");
}

// @internal
#[test]
fn overview_configured_offers_send_and_disable() {
    let engine = EmergencyBroadcastEngine::new(Some(configured()));
    let screen = engine.current_screen();
    let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert!(ids.contains(&"configure"));
    assert!(ids.contains(&"send"));
    assert!(ids.contains(&"disable"));
}

// @internal
#[test]
fn configure_flow_saves_with_save_outcome() {
    let mut engine = EmergencyBroadcastEngine::new(None);

    // configure → contacts screen
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    assert!(matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_contacts"));

    // empty contacts → validation error
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(&r, ActionResult::ValidationError { component_id, .. } if component_id == "contact_ids"),
        "empty contacts must fail, got {r:?}"
    );

    // paste contacts → continue to message
    engine.handle_action(UserAction::TextChanged {
        component_id: "contact_ids".into(),
        value: "abc, def".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_message"));

    // edit message, toggle location, save → Complete with Save outcome
    engine.handle_action(UserAction::TextChanged {
        component_id: "message".into(),
        value: "I need help".into(),
    });
    engine.handle_action(UserAction::ItemToggled {
        component_id: "options".into(),
        item_id: "include_location".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert!(
        matches!(r, ActionResult::Complete),
        "save completes, got {r:?}"
    );
    assert_eq!(engine.outcome(), Some(&EmergencyOutcome::Save));
    assert_eq!(
        engine.contact_ids(),
        vec!["abc".to_string(), "def".to_string()]
    );
    assert_eq!(engine.message(), "I need help");
    assert!(engine.include_location());
}

// @internal
#[test]
fn too_many_contacts_is_rejected() {
    let mut engine = EmergencyBroadcastEngine::new(None);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    // 11 contacts > MAX_TRUSTED_CONTACTS (10)
    let many = (0..11)
        .map(|i| format!("c{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    engine.handle_action(UserAction::TextChanged {
        component_id: "contact_ids".into(),
        value: many,
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(&r, ActionResult::ValidationError { component_id, .. } if component_id == "contact_ids"),
        "over-limit must fail, got {r:?}"
    );
}

// @internal
#[test]
fn send_flow_requires_confirmation_and_sets_send_outcome() {
    let mut engine = EmergencyBroadcastEngine::new(Some(configured()));

    // send → confirm screen (InlineConfirm)
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "send".into(),
    });
    assert!(
        matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_confirm_send"),
        "send opens confirmation, got {r:?}"
    );

    // cancel returns to overview without sending
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_send".into(),
    });
    assert!(matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_overview"));
    assert_eq!(engine.outcome(), None, "cancel must not set an outcome");

    // send again, confirm → Complete with Send outcome
    engine.handle_action(UserAction::ActionPressed {
        action_id: "send".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_send".into(),
    });
    assert!(
        matches!(r, ActionResult::Complete),
        "confirm sends, got {r:?}"
    );
    assert_eq!(engine.outcome(), Some(&EmergencyOutcome::Send));
}

// @internal
#[test]
fn disable_requires_confirmation_and_sets_disable_outcome() {
    let mut engine = EmergencyBroadcastEngine::new(Some(configured()));

    // disable → InlineConfirm appears on overview
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
    assert!(matches!(r, ActionResult::UpdateScreen(_)));
    let has_confirm = engine
        .current_screen()
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { id, .. } if id == "disable"));
    assert!(has_confirm, "disable shows an InlineConfirm");

    // cancel clears it, no outcome
    engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_disable".into(),
    });
    assert_eq!(engine.outcome(), None);

    // disable + confirm → Complete with Disable outcome
    engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_disable".into(),
    });
    assert!(matches!(r, ActionResult::Complete));
    assert_eq!(engine.outcome(), Some(&EmergencyOutcome::Disable));
}

// @internal
#[test]
fn textinput_enter_submit_advances_like_continue_and_save() {
    // Keyboard frontends emit `submit_<id>` when Enter is pressed on a
    // non-empty TextInput (the FormDialog convention). The engine must treat
    // those as continue / save so a typed field can advance via Enter.
    let mut engine = EmergencyBroadcastEngine::new(None);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    engine.handle_action(UserAction::TextChanged {
        component_id: "contact_ids".into(),
        value: "abc".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_contact_ids".into(),
    });
    assert!(
        matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_message"),
        "submit_contact_ids must advance like continue, got {r:?}"
    );
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_message".into(),
    });
    assert!(
        matches!(r, ActionResult::Complete),
        "submit_message saves, got {r:?}"
    );
    assert_eq!(engine.outcome(), Some(&EmergencyOutcome::Save));
}

// @internal
#[test]
fn captured_values_are_reflected_for_keyboard_frontends() {
    let mut engine = EmergencyBroadcastEngine::new(None);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    engine.handle_action(UserAction::TextChanged {
        component_id: "contact_ids".into(),
        value: "x, y".into(),
    });
    let reflected = engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::TextInput { id, value, .. } if id == "contact_ids" => Some(value.clone()),
            _ => None,
        });
    assert_eq!(
        reflected.as_deref(),
        Some("x, y"),
        "contact_ids must reflect input"
    );
}
