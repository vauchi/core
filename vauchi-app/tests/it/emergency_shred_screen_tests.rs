// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The emergency wipe is one screen (owner decision 2026-09-29, #419
//! item 4): the warning, a "Type WIPE to confirm" field, and a
//! destructive Shred Everything / Cancel pair in the body. The commands
//! used to be contextual actions, which shells fold into the Actions
//! menu, so the one button that matters was hidden.

use vauchi_app::ui::{
    ActionResult, Component, EmergencyShredEngine, EngineOutput, GdprChoice, UserAction,
    WorkflowEngine,
};

fn engine() -> EmergencyShredEngine {
    EmergencyShredEngine::new(vauchi_app::i18n::Locale::English)
}

fn type_word(engine: &mut EmergencyShredEngine, word: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: word.into(),
    });
}

fn press(engine: &mut EmergencyShredEngine, id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: id.into(),
    })
}

// @internal
#[test]
fn the_wipe_screen_holds_the_warning_the_field_and_the_buttons() {
    let screen = engine().current_screen();

    assert_eq!(screen.title, "Emergency Data Wipe");
    assert_eq!(
        screen.subtitle.as_deref(),
        Some(
            "Immediately destroys all data on this device and asks your contacts' apps to delete your card."
        )
    );
    assert!(matches!(
        &screen.components[0],
        Component::InfoPanel { items, .. } if items.len() == 3
    ));
    assert!(matches!(
        &screen.components[1],
        Component::TextInput { id, label, .. }
            if id == "confirmation" && label == "Type WIPE to confirm"
    ));
    match &screen.components[2] {
        Component::InlineConfirm {
            confirm_text,
            cancel_text,
            confirm_action_id,
            cancel_action_id,
            destructive,
            ..
        } => {
            assert_eq!(confirm_text, "Shred Everything");
            assert_eq!(cancel_text, "Cancel");
            assert_eq!(confirm_action_id, "confirm_shred");
            assert_eq!(cancel_action_id, "cancel_shred");
            assert!(*destructive);
        }
        other => panic!("expected the Shred Everything / Cancel pair, got {other:?}"),
    }
    assert!(
        screen.contextual_actions.is_empty(),
        "contextual actions end up in the Actions menu"
    );
}

// @internal
#[test]
fn the_word_wipe_confirms_however_the_keyboard_typed_it() {
    for typed in ["WIPE", "wipe", "Wipe", " WIPE "] {
        let mut engine = engine();
        type_word(&mut engine, typed);

        assert_eq!(
            press(&mut engine, "confirm_shred"),
            ActionResult::Complete,
            "{typed:?}"
        );
        assert!(
            matches!(
                engine.engine_output(),
                Some(EngineOutput::Gdpr(GdprChoice::Shred))
            ),
            "{typed:?} should hand the wipe to the app"
        );
    }
}

// @internal
#[test]
fn any_other_word_is_refused_on_the_field() {
    for typed in ["", "WIP", "WIPED", "DELETE", "wipe it", "WIPE\0"] {
        let mut engine = engine();
        type_word(&mut engine, typed);

        assert_eq!(
            press(&mut engine, "confirm_shred"),
            ActionResult::ValidationError {
                component_id: "confirmation".into(),
                message: "Type the word WIPE to shred everything.".into(),
            },
            "{typed:?}"
        );
        assert!(
            engine.engine_output().is_none(),
            "{typed:?} handed off a wipe"
        );
    }
}
