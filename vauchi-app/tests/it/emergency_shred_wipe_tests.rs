// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings → Emergency Wipe must destroy the data it says it destroys.
//! Someone reaching for it may be under coercion; a screen that reports
//! "wiping" while the identity survives is worse than no button.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_on_shred() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Settings);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    engine
}

fn press(engine: &mut AppEngine, id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: id.into(),
    })
}

// @internal
#[test]
fn confirming_the_emergency_wipe_destroys_the_identity() {
    let mut engine = engine_on_shred();
    let _ = press(&mut engine, "continue");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "DELETE".into(),
    });
    let result = press(&mut engine, "wipe");

    assert!(
        matches!(result, ActionResult::WipeComplete),
        "expected the app to reset after the wipe, got {result:?}"
    );
    assert!(engine.vauchi().identity().is_none(), "identity survived");
}

// @internal
#[test]
fn cancelling_the_emergency_wipe_keeps_the_data_and_returns_to_settings() {
    let mut engine = engine_on_shred();
    let _ = press(&mut engine, "cancel");

    assert_eq!(engine.current_app_screen(), &AppScreen::Settings);
    assert!(engine.vauchi().identity().is_some());
}
