// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn engine() -> EmergencyShredEngine {
    EmergencyShredEngine::new(vauchi_app::i18n::Locale::English)
}

fn type_word(engine: &mut EmergencyShredEngine, word: &str) -> ActionResult {
    engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: word.into(),
    })
}

fn press(engine: &mut EmergencyShredEngine, id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: id.into(),
    })
}

fn confirmation_error(screen: &ScreenModel) -> Option<String> {
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
fn shred_is_one_screen_without_a_step_counter() {
    let screen = engine().current_screen();
    assert_eq!(screen.screen_id, "shred_warning");
    assert_eq!(screen.progress, None);
}

// @scenario: emergency_shred :: Hard shred requires valid shred token
// @internal
#[test]
fn shred_keeps_the_refusal_on_the_field_until_the_user_types_again() {
    let mut engine = engine();
    let _ = press(&mut engine, "confirm_shred");
    assert_eq!(
        confirmation_error(&engine.current_screen()).as_deref(),
        Some("Type the word WIPE to shred everything.")
    );

    let _ = type_word(&mut engine, "W");
    assert_eq!(confirmation_error(&engine.current_screen()), None);
    assert!(engine.engine_output().is_none());
}

// @scenario: emergency_shred :: Panic shred destroys everything immediately
// @internal
#[test]
fn shred_confirm_wipe_hands_the_wipe_to_the_app() {
    // The engine cannot wipe; the AppEngine does on completion (#431).
    let mut engine = engine();
    let _ = type_word(&mut engine, "WIPE");

    assert_eq!(press(&mut engine, "confirm_shred"), ActionResult::Complete);
    assert!(matches!(
        engine.engine_output(),
        Some(EngineOutput::Gdpr(GdprChoice::Shred))
    ));
}

// @internal
#[test]
fn shred_cancel_after_confirming_hands_off_no_wipe() {
    let mut engine = engine();
    let _ = type_word(&mut engine, "WIPE");
    let _ = press(&mut engine, "confirm_shred");

    assert_eq!(press(&mut engine, "cancel_shred"), ActionResult::Complete);
    assert!(engine.engine_output().is_none());
}

// @scenario: emergency_shred :: Cancel soft shred during grace period
// @internal
#[test]
fn shred_cancel_returns_complete() {
    let mut engine = engine();
    assert_eq!(press(&mut engine, "cancel_shred"), ActionResult::Complete);
    assert!(engine.engine_output().is_none());
}

// Proptest counterexample (shred_hands_off_a_wipe_only_for_the_typed_word):
// after a failed wipe the user stays on the screen, and changing the word
// must withdraw the confirmation.
// @internal
#[test]
fn shred_editing_the_word_after_confirming_withdraws_the_wipe() {
    let mut engine = engine();
    let _ = type_word(&mut engine, "WIPE");
    let _ = press(&mut engine, "confirm_shred");
    let _ = type_word(&mut engine, "123456");

    assert!(engine.engine_output().is_none());
}
