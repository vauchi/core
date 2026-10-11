// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A duress unlock sends nothing, so duress setup asks for no alert
//! recipients (ADR-032 amendment 2026-10-11, vauchi/private#636).

use vauchi_app::i18n::Locale;
use vauchi_app::ui::info_content::resolve_info_key;
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
    let engine = DuressPinEngine::new(DuressConfig { enabled: true }, Locale::English);

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

fn visible_text(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => into.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| visible_text(item, into)),
        serde_json::Value::Object(fields) => fields
            .iter()
            .filter(|(key, _)| key.as_str() != "role")
            .for_each(|(_, field)| visible_text(field, into)),
        _ => {}
    }
}

fn screen_copy(engine: &DuressPinEngine) -> String {
    let screen = serde_json::to_value(engine.current_screen()).expect("the screen serializes");
    let mut text = Vec::new();
    visible_text(&screen, &mut text);
    text.join(" ")
}

/// Every duress string a user reads in setup, the overview, the
/// "how it looks" sheet, the disable confirmation and the info sheet.
fn all_duress_copy() -> Vec<String> {
    let mut copy = Vec::new();

    let mut setup = DuressPinEngine::new(DuressConfig::default(), Locale::English);
    copy.push(screen_copy(&setup));
    let _ = select(&mut setup, "duress_actions", "set_up");
    copy.push(screen_copy(&setup));
    type_into(&mut setup, "pin", "246813");
    let _ = press(&mut setup, "continue");
    copy.push(screen_copy(&setup));

    let mut enabled = DuressPinEngine::new(DuressConfig { enabled: true }, Locale::English);
    copy.push(screen_copy(&enabled));
    if let ActionResult::ShowInfoOverlay { title, body } =
        select(&mut enabled, "duress_actions", "how_it_looks")
    {
        copy.push(format!("{title} {body}"));
    } else {
        panic!("how_it_looks opens an info sheet");
    }
    let _ = select(&mut enabled, "duress_actions", "turn_off");
    copy.push(screen_copy(&enabled));

    let (title, body) = resolve_info_key("duress_pin", Locale::English).expect("the info sheet");
    copy.push(format!("{title} {body}"));
    copy
}

// @scenario: duress_mode :: A duress unlock sends nothing
#[test]
fn no_duress_copy_promises_an_alert() {
    let promises: Vec<String> = all_duress_copy()
        .into_iter()
        .filter(|text| {
            text.to_lowercase()
                .replace("alerts no one", "")
                .contains("alert")
        })
        .collect();

    assert!(
        promises.is_empty(),
        "duress copy must not promise an alert: {promises:#?}"
    );
}
