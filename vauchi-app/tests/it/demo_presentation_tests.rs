// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later
//! The web demo's reducer answers every refused event with Core's prepared
//! alert, as `AppEngine` does for the other shells, so the WASM shell never
//! shows an error value's text (ADR-045 Am1, vauchi/private#412).
use serde_json::json;
use vauchi_app::i18n::{Locale, get_string, get_string_with_args};
use vauchi_app::ui::DemoPresentationEngine;
use vauchi_core::{AlertSpec, Command, MAX_EVENT_INPUT_VALUE_BYTES};

fn onboarding_demo() -> DemoPresentationEngine {
    let mut engine = DemoPresentationEngine::new("onboarding").expect("onboarding demo");
    engine.initial_commands().expect("initial commands");
    engine
}

fn alert_with(message: String) -> Vec<Command> {
    vec![Command::PresentAlert {
        alert: AlertSpec {
            title: get_string(Locale::English, "error.title"),
            message,
        },
    }]
}

fn generic_alert() -> Vec<Command> {
    alert_with(get_string(Locale::English, "error.generic"))
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn an_event_for_a_surface_that_is_not_shown_gets_the_generic_alert() {
    let event = json!({
        "ActionActivated": {
            "surface_id": "demo.not-shown",
            "interaction_id": "demo.nothing-here",
        }
    })
    .to_string();

    assert_eq!(onboarding_demo().dispatch_json(&event), generic_alert());
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn json_that_is_not_an_event_gets_the_generic_alert() {
    for payload in ["", "{not json", "[]", r#"{"NoSuchEvent":{}}"#] {
        assert_eq!(
            onboarding_demo().dispatch_json(payload),
            generic_alert(),
            "payload {payload:?}"
        );
    }
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn an_over_bound_input_value_is_refused_naming_the_bound() {
    let event = json!({
        "ValueChanged": {
            "surface_id": "main",
            "binding_id": "display-name",
            "value": {"text": "x".repeat(MAX_EVENT_INPUT_VALUE_BYTES + 1)},
        }
    })
    .to_string();

    assert_eq!(
        onboarding_demo().dispatch_json(&event),
        alert_with(get_string_with_args(
            Locale::English,
            "validation.too_long",
            &[("max", &MAX_EVENT_INPUT_VALUE_BYTES.to_string())],
        ))
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Available window drives structural composition
// @scenario: generic_presentation_protocol.feature :: Available window drives structural composition
#[test]
fn an_accepted_event_is_answered_with_its_commands_not_an_alert() {
    let event = json!({
        "PresentationEnvironmentChanged": {
            "available_width": 700,
            "available_height": 900,
            "input_modes": ["touch"],
            "motion": "full",
        }
    })
    .to_string();

    let commands = onboarding_demo().dispatch_json(&event);

    assert!(!commands.is_empty(), "an accepted event yields commands");
    assert!(
        commands
            .iter()
            .all(|command| !matches!(command, Command::PresentAlert { .. })),
        "an accepted event must not be answered with an alert: {commands:?}"
    );
}
