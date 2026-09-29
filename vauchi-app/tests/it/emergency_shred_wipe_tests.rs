// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings → Emergency Wipe must destroy the data it says it destroys.
//! Someone reaching for it may be under coercion; a screen that reports
//! "wiping" while the identity survives is worse than no button.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
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

fn confirmation_error(result: &ActionResult) -> Option<String> {
    let ActionResult::UpdateScreen(screen) = result else {
        return None;
    };
    screen.components.iter().find_map(|c| match c {
        Component::TextInput {
            id,
            validation_error,
            ..
        } if id == "confirmation" => validation_error.clone(),
        _ => None,
    })
}

// @internal
#[test]
fn confirming_the_emergency_wipe_destroys_the_identity() {
    let mut engine = engine_on_shred();
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "WIPE".into(),
    });
    let result = press(&mut engine, "confirm_shred");

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
    let _ = press(&mut engine, "cancel_shred");

    assert_eq!(engine.current_app_screen(), &AppScreen::Settings);
    assert!(engine.vauchi().identity().is_some());
}

// @internal
#[test]
fn a_near_miss_confirmation_word_wipes_nothing() {
    for typed in ["", "DELETE", "WIPED"] {
        let mut engine = engine_on_shred();
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "confirmation".into(),
            value: typed.into(),
        });
        let result = press(&mut engine, "confirm_shred");

        assert_eq!(
            confirmation_error(&result).as_deref(),
            Some("Type the word WIPE to shred everything."),
            "{typed:?} should be refused on the field"
        );
        assert!(
            engine.vauchi().identity().is_some(),
            "{typed:?} wiped the identity"
        );
    }
}
