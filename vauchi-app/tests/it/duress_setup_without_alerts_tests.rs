// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A duress unlock sends nothing, so duress setup asks for no alert
//! recipients (ADR-032 amendment 2026-10-11, vauchi/private#636).

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, Component, DuressConfig, DuressPinEngine, EngineOutput, UserAction,
    WorkflowEngine,
};

fn select(engine: &mut DuressPinEngine, component_id: &str, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: component_id.into(),
        item_id: item_id.into(),
    })
}

fn type_into(engine: &mut DuressPinEngine, field: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: field.into(),
        value: value.into(),
    });
}

fn press(engine: &mut DuressPinEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

// @scenario: duress_mode :: A duress unlock sends nothing
#[test]
fn confirming_the_pin_completes_the_setup() {
    let mut engine = DuressPinEngine::new(DuressConfig::default(), Locale::English);
    let _ = select(&mut engine, "duress_actions", "set_up");
    type_into(&mut engine, "pin", "246813");
    let _ = press(&mut engine, "continue");
    type_into(&mut engine, "confirm_pin", "246813");

    let result = press(&mut engine, "continue");

    assert!(
        matches!(result, ActionResult::Complete),
        "setup ends at the confirmed PIN, got {result:?}"
    );
    match engine.engine_output() {
        Some(EngineOutput::DuressPin(setup)) => {
            assert!(setup.enabled);
            assert_eq!(setup.pin, "246813");
        }
        other => panic!("expected the duress PIN output, got {other:?}"),
    }
}

// @scenario: duress_mode :: A duress unlock sends nothing
#[test]
fn setup_counts_two_steps() {
    let mut engine = DuressPinEngine::new(DuressConfig::default(), Locale::English);
    let _ = select(&mut engine, "duress_actions", "set_up");

    let progress = engine.current_screen().progress.expect("a step bar");

    assert_eq!((progress.current_step, progress.total_steps), (1, 2));
}

// @scenario: duress_mode :: A duress unlock sends nothing
#[test]
fn an_enabled_duress_pin_lists_no_alerts_row() {
    let engine = DuressPinEngine::new(
        DuressConfig {
            enabled: true,
            ..DuressConfig::default()
        },
        Locale::English,
    );

    let rows: Vec<String> = engine
        .current_screen()
        .components
        .iter()
        .find_map(|component| match component {
            Component::ActionList { id, items, .. } if id == "duress_rows" => {
                Some(items.iter().map(|item| item.id.clone()).collect())
            }
            _ => None,
        })
        .expect("the duress rows");

    assert_eq!(rows, vec!["pin", "decoys"]);
}
