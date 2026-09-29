// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// @internal
#[test]
fn shred_starts_at_warning() {
    let engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "shred_warning");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 1);
    assert_eq!(screen.progress.as_ref().unwrap().total_steps, 2);
}

// @internal
#[test]
fn shred_warning_has_info_panel() {
    let engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let screen = engine.current_screen();

    let info_panel = screen.components.first().expect("should have a component");
    match info_panel {
        Component::InfoPanel {
            icon, title, items, ..
        } => {
            assert_eq!(icon.as_deref(), Some("warning"));
            assert_eq!(title, "Emergency Data Wipe");
            assert_eq!(items.len(), 3);
        }
        other => panic!("expected InfoPanel, got {:?}", other),
    }

    assert_eq!(screen.contextual_actions.len(), 2);
    assert_eq!(screen.contextual_actions[0].id, "continue");
    assert_eq!(screen.contextual_actions[0].style, ActionStyle::Destructive);
    assert_eq!(screen.contextual_actions[0].label, "I Understand");
    assert_eq!(screen.contextual_actions[1].id, "cancel");
    assert_eq!(screen.contextual_actions[1].style, ActionStyle::Secondary);
}

// @internal
#[test]
fn shred_continue_to_confirm() {
    let mut engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "shred_confirm");
            assert_eq!(screen.progress.as_ref().unwrap().current_step, 2);
        }
        other => panic!("expected NavigateTo, got {:?}", other),
    }
}

// @scenario: emergency_shred :: Hard shred requires valid shred token
// @internal
#[test]
fn shred_confirm_requires_delete_text() {
    let mut engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    let wipe_action = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "wipe")
        .expect("should have wipe action");
    assert!(
        !wipe_action.enabled,
        "wipe should be disabled without DELETE text"
    );

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "DELETE".into(),
    });
    let screen = engine.current_screen();
    let wipe_action = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "wipe")
        .expect("should have wipe action");
    assert!(
        wipe_action.enabled,
        "wipe should be enabled with DELETE text"
    );
}

// @internal
#[test]
fn shred_confirm_wrong_text_validation_error() {
    let mut engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "WRONG".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });

    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "confirmation");
            assert_eq!(message, "Type DELETE to confirm");
        }
        other => panic!("expected ValidationError, got {:?}", other),
    }
}

// @scenario: emergency_shred :: Panic shred destroys everything immediately
// @internal
#[test]
fn shred_confirm_delete_hands_the_wipe_to_the_app() {
    // The engine cannot wipe; the AppEngine does on completion (#431).
    let mut engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "DELETE".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });

    assert_eq!(result, ActionResult::Complete);
    assert!(matches!(
        engine.engine_output(),
        Some(EngineOutput::Gdpr(GdprChoice::Shred))
    ));
}

// @internal
#[test]
fn shred_cancel_after_confirming_hands_off_no_wipe() {
    let mut engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "DELETE".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });
    assert_eq!(result, ActionResult::Complete);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert_eq!(result, ActionResult::Complete);
    assert!(engine.engine_output().is_none());
}

// @scenario: emergency_shred :: Cancel soft shred during grace period
// @internal
#[test]
fn shred_cancel_returns_complete() {
    let mut engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert_eq!(result, ActionResult::Complete);

    let mut engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}
