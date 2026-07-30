// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M2 S7 (record `2026-06-02-exchange-group-selection-ux` D): the group
//! selection screen gets a Cancel action that exits the exchange, and the
//! `continue`/`skip` pair collapses into one primary button whose label is
//! "Skip" with nothing selected and "Continue" with ≥1 group selected.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_on_group_selection() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let work = vauchi.create_group("Work").expect("create group");
    let work_id = work.id().to_string();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_group_selection"
    );
    (engine, work_id)
}

fn action_labels(engine: &AppEngine) -> Vec<(String, String)> {
    engine
        .current_screen()
        .contextual_actions
        .iter()
        .map(|a| (a.id.clone(), a.label.clone()))
        .collect()
}

// One primary button: "Skip" at 0 selected, "Continue" at ≥1 — plus Cancel.
// @scenario: exchange :: group selection has one primary button and Cancel
// @internal
#[test]
fn single_primary_button_flips_label_with_selection() {
    let (mut engine, work_id) = engine_on_group_selection();

    let actions = action_labels(&engine);
    assert_eq!(
        actions,
        vec![
            ("continue".to_string(), "Skip".to_string()),
            ("cancel".to_string(), "Cancel".to_string()),
        ],
        "0 selected: one primary button labeled Skip, plus Cancel"
    );

    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".into(),
        item_id: work_id.clone(),
    });
    let actions = action_labels(&engine);
    assert_eq!(
        actions,
        vec![
            ("continue".to_string(), "Continue".to_string()),
            ("cancel".to_string(), "Cancel".to_string()),
        ],
        "≥1 selected: the same button reads Continue"
    );

    // Toggling back off flips the label back.
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".into(),
        item_id: work_id,
    });
    assert_eq!(
        action_labels(&engine)[0].1,
        "Skip",
        "deselecting flips the label back to Skip"
    );
}

// Cancel exits the exchange (routes off the exchange flow entirely).
// @scenario: exchange :: Cancel on group selection exits the exchange
// @internal
#[test]
fn cancel_exits_the_exchange() {
    let (mut engine, _work) = engine_on_group_selection();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert_ne!(
        engine.current_screen().screen_id,
        "exchange_group_selection",
        "Cancel must leave the group selection screen"
    );
    assert!(
        !engine.current_screen().screen_id.starts_with("exchange_"),
        "Cancel exits the exchange flow, got {}",
        engine.current_screen().screen_id
    );
}

// The unified button keeps the old semantics: with a selection it arms the
// field preview (shown after the mode pick); with none it skips straight
// through — pinned end-to-end.
// @scenario: exchange :: unified button keeps continue/skip semantics
// @internal
#[test]
fn unified_button_routes_by_selection_count() {
    // With a selection: preview appears after picking a mode.
    let (mut engine, work_id) = engine_on_group_selection();
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".into(),
        item_id: work_id,
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "hero".into(),
        item_id: "mode:hover".into(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_field_preview",
        "with groups selected, Continue arms the field preview"
    );

    // Without a selection: no preview — straight to the mode sub-flow.
    let (mut engine, _work) = engine_on_group_selection();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "hero".into(),
        item_id: "mode:hover".into(),
    });
    assert_ne!(
        engine.current_screen().screen_id,
        "exchange_field_preview",
        "with nothing selected, the unified button skips the preview"
    );
}
