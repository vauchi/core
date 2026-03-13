// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

fn default_config() -> DuressConfig {
    DuressConfig {
        enabled: false,
        alert_contacts: vec![],
        alert_message: String::new(),
        include_location: false,
    }
}

fn enabled_config() -> DuressConfig {
    DuressConfig {
        enabled: true,
        alert_contacts: vec![],
        alert_message: "Help me".into(),
        include_location: true,
    }
}

#[test]
fn duress_starts_at_overview() {
    let engine = DuressPinEngine::new(default_config());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "duress_overview");
    assert_eq!(
        screen
            .progress
            .as_ref()
            .expect("should have progress")
            .current_step,
        1
    );
    assert_eq!(
        screen
            .progress
            .as_ref()
            .expect("should have progress")
            .total_steps,
        4
    );
}

#[test]
fn duress_overview_shows_enable_toggle() {
    let engine = DuressPinEngine::new(default_config());
    let screen = engine.current_screen();

    let toggle_list = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::ToggleList { id, .. } if id == "duress_toggle"))
        .expect("should have a ToggleList for duress toggle");

    match toggle_list {
        Component::ToggleList { items, .. } => {
            let enabled_item = items
                .iter()
                .find(|i| i.id == "enabled")
                .expect("should have 'enabled' toggle item");
            assert!(
                !enabled_item.selected,
                "enabled toggle should be off for default config"
            );
        }
        _ => unreachable!(),
    }

    // When disabled, should show "Set Up PIN" and no disable action
    let configure = screen
        .actions
        .iter()
        .find(|a| a.id == "configure")
        .expect("should have configure action");
    assert_eq!(configure.label, "Set Up PIN");
    assert!(
        screen.actions.iter().all(|a| a.id != "disable"),
        "disable action should not appear when not enabled"
    );

    // When enabled, should show "Change PIN" and a disable action
    let engine = DuressPinEngine::new(enabled_config());
    let screen = engine.current_screen();
    let configure = screen
        .actions
        .iter()
        .find(|a| a.id == "configure")
        .expect("should have configure action");
    assert_eq!(configure.label, "Change PIN");
    let disable = screen
        .actions
        .iter()
        .find(|a| a.id == "disable")
        .expect("should have disable action when enabled");
    assert_eq!(disable.style, ActionStyle::Destructive);
}

#[test]
fn duress_configure_goes_to_pin() {
    let mut engine = DuressPinEngine::new(default_config());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "duress_enter_pin");
            assert_eq!(screen.progress.as_ref().expect("progress").current_step, 2);
        }
        other => panic!("expected NavigateTo, got {:?}", other),
    }
}

#[test]
fn duress_enter_pin_validation() {
    let mut engine = DuressPinEngine::new(default_config());
    // Navigate to EnterPin
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });

    // Try to continue with empty PIN
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "pin");
            assert_eq!(message, "Please enter a PIN");
        }
        other => panic!("expected ValidationError, got {:?}", other),
    }

    // Enter a PIN via TextChanged
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "TextChanged should return UpdateScreen"
    );

    // Now continue should succeed
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "duress_confirm_pin");
        }
        other => panic!("expected NavigateTo confirm, got {:?}", other),
    }
}

#[test]
fn duress_pin_mismatch_error() {
    let mut engine = DuressPinEngine::new(default_config());
    // Navigate to EnterPin, enter PIN, advance to ConfirmPin
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Enter a different confirm PIN
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_pin".into(),
        value: "654321".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "confirm_pin");
            assert_eq!(message, "PINs do not match");
        }
        other => panic!("expected ValidationError for mismatch, got {:?}", other),
    }
}

#[test]
fn duress_pin_match_to_alerts() {
    let mut engine = DuressPinEngine::new(default_config());
    // Navigate through EnterPin → ConfirmPin with matching PINs
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_pin".into(),
        value: "123456".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "duress_alerts");
            assert_eq!(screen.progress.as_ref().expect("progress").current_step, 4);
        }
        other => panic!("expected NavigateTo alerts, got {:?}", other),
    }
}

#[test]
fn duress_alerts_save_enables() {
    let mut engine = DuressPinEngine::new(default_config());
    assert!(!engine.config().enabled, "should start disabled");

    // Navigate all the way to ConfigureAlerts
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Update alert message
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "alert_message".into(),
        value: "I need help".into(),
    });
    assert_eq!(engine.config().alert_message, "I need help");

    // Toggle include_location
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "alerts".into(),
        item_id: "include_location".into(),
    });
    assert!(
        engine.config().include_location,
        "include_location should be toggled on"
    );

    // Save
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert_eq!(result, ActionResult::Complete);
    assert!(
        engine.config().enabled,
        "config should be enabled after save"
    );
}

#[test]
fn duress_disable_shows_inline_confirm() {
    let mut engine = DuressPinEngine::new(enabled_config());
    assert!(engine.config().enabled, "should start enabled");

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
    let screen = match result {
        ActionResult::UpdateScreen(s) => s,
        other => panic!("Expected UpdateScreen, got {:?}", other),
    };
    let has_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { destructive, .. } if *destructive));
    assert!(
        has_confirm,
        "disable should show a destructive InlineConfirm"
    );
    // Config should still be enabled until confirmed
    assert!(
        engine.config().enabled,
        "should remain enabled until confirmed"
    );
}

#[test]
fn duress_confirm_disable_completes() {
    let mut engine = DuressPinEngine::new(enabled_config());
    // Show confirmation
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
    // Confirm disable
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_disable".into(),
    });
    assert_eq!(
        result,
        ActionResult::Complete,
        "confirm should return Complete"
    );
    assert!(
        !engine.config().enabled,
        "config should be disabled after confirm"
    );
}

#[test]
fn duress_cancel_disable_keeps_enabled() {
    let mut engine = DuressPinEngine::new(enabled_config());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_disable".into(),
    });
    let screen = match result {
        ActionResult::UpdateScreen(s) => s,
        other => panic!("Expected UpdateScreen, got {:?}", other),
    };
    let has_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { .. }));
    assert!(!has_confirm, "cancel should remove InlineConfirm");
    assert!(
        engine.config().enabled,
        "config should remain enabled after cancel"
    );
}

#[test]
fn duress_back_navigation() {
    let mut engine = DuressPinEngine::new(default_config());

    // Overview → EnterPin
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "duress_enter_pin");

    // EnterPin → back → Overview
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    match &result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "duress_overview");
        }
        other => panic!("expected NavigateTo overview, got {:?}", other),
    }

    // Navigate forward to ConfirmPin
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "duress_confirm_pin");

    // ConfirmPin → back → EnterPin
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    match &result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "duress_enter_pin");
        }
        other => panic!("expected NavigateTo enter_pin, got {:?}", other),
    }

    // Navigate forward again to ConfigureAlerts
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "duress_alerts");

    // ConfigureAlerts → back → ConfirmPin
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    match &result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "duress_confirm_pin");
        }
        other => panic!("expected NavigateTo confirm_pin, got {:?}", other),
    }
}

#[test]
fn duress_pin_accumulates_single_chars() {
    let mut engine = DuressPinEngine::new(default_config());
    // Navigate to EnterPin
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });

    // Type "1", "2", "3", "4", "5", "6" one char at a time
    for ch in ['1', '2', '3', '4', '5', '6'] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: ch.to_string(),
        });
    }

    // Continue should succeed (PIN is non-empty)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "duress_confirm_pin",
                "should advance to confirm after accumulating 6 chars"
            );
        }
        other => panic!("expected NavigateTo, got {:?}", other),
    }

    // Now type the same PIN in confirm step, one char at a time
    for ch in ['1', '2', '3', '4', '5', '6'] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "confirm_pin".into(),
            value: ch.to_string(),
        });
    }

    // Continue should succeed (PINs match)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "duress_alerts",
                "should advance to alerts when accumulated PINs match"
            );
        }
        other => panic!("expected NavigateTo alerts, got {:?}", other),
    }
}

#[test]
fn duress_pin_backspace_removes_last_char() {
    let mut engine = DuressPinEngine::new(default_config());
    // Navigate to EnterPin
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });

    // Type "1", "2", "3"
    for ch in ['1', '2', '3'] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: ch.to_string(),
        });
    }

    // Backspace
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: String::new(),
    });

    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::PinInput { id, .. } if id == "pin"))
        .expect("should have PinInput");
    match pin {
        Component::PinInput { filled, .. } => {
            assert_eq!(
                *filled, 2,
                "should have 2 filled positions after typing 123 then backspace"
            );
        }
        _ => unreachable!(),
    }
}
