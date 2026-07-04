// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn default_config() -> DuressConfig {
    DuressConfig {
        enabled: false,
        available_contacts: vec![],
        selected_contact_ids: vec![],
        alert_message: String::new(),
        include_location: false,
    }
}

fn enabled_config() -> DuressConfig {
    DuressConfig {
        enabled: true,
        available_contacts: vec![],
        selected_contact_ids: vec![],
        alert_message: "Help me".into(),
        include_location: true,
    }
}

fn contact_item(id: &str, name: &str) -> Item {
    Item {
        id: id.into(),
        name: name.into(),
        subtitle: None,
        avatar_initials: name
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_default(),
        status: None,
        actions: vec![],
        a11y: None,
    }
}

/// A config whose picker offers two contacts, none selected yet.
fn config_with_contacts() -> DuressConfig {
    DuressConfig {
        enabled: false,
        available_contacts: vec![contact_item("c1", "Alice"), contact_item("c2", "Bob")],
        selected_contact_ids: vec![],
        alert_message: String::new(),
        include_location: false,
    }
}

/// Drive a fresh engine through the PIN steps to the ConfigureAlerts screen.
fn engine_at_alerts(config: DuressConfig) -> DuressPinEngine {
    let mut engine = DuressPinEngine::new(config);
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
    engine
}

// @scenario: duress_mode :: Duress mode is opt-in and disabled by default
// @internal
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

// The overview status must be a read-only StatusIndicator, not an
// interactive ToggleList the engine snaps back (a control that cannot
// act — 2026-07-03-coercion-safety-config-gaps defect 3).
// @internal
#[test]
fn duress_overview_shows_read_only_status_not_dead_toggle() {
    let engine = DuressPinEngine::new(default_config());
    let screen = engine.current_screen();

    assert!(
        !screen.components.iter().any(|c| matches!(
            c,
            Component::ToggleList { id, .. } if id == "duress_toggle"
        )),
        "overview must not render a dead interactive toggle for status"
    );
    let status = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::StatusIndicator { id, status, .. } if id == "duress_status" => {
                Some(status.clone())
            }
            _ => None,
        })
        .expect("overview must show a read-only StatusIndicator for enabled state");
    assert_eq!(
        status,
        Status::Warning,
        "default (not set up) duress must surface a Warning status"
    );

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

// @internal
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

// @internal
#[test]
fn duress_enter_pin_validation() {
    let mut engine = DuressPinEngine::new(default_config());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });

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

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "TextChanged should return UpdateScreen"
    );

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

// @scenario: duress_mode :: Duress PIN must differ from normal PIN
// @internal
#[test]
fn duress_pin_mismatch_error() {
    let mut engine = DuressPinEngine::new(default_config());
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

// @internal
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

// @scenario: duress_mode :: Enable duress PIN in settings
// @scenario: duress_mode :: Configure trusted contacts for duress alerts
// @internal
#[test]
fn duress_alerts_save_enables() {
    let mut engine = engine_at_alerts(config_with_contacts());
    assert!(!engine.config().enabled, "should start disabled");

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "alert_message".into(),
        value: "I need help".into(),
    });
    assert_eq!(engine.config().alert_message, "I need help");

    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "alerts".into(),
        item_id: "include_location".into(),
    });
    assert!(
        engine.config().include_location,
        "include_location should be toggled on"
    );

    // A recipient must be chosen before the alert can be saved.
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "recipients".into(),
        item_id: "c1".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert_eq!(result, ActionResult::Complete);
    assert!(
        engine.config().enabled,
        "config should be enabled after save"
    );
}

// @internal
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
    let has_confirm = screen.components.iter().any(|c| {
        matches!(c, Component::InlineConfirm { destructive, ..
        } if *destructive)
    });
    assert!(
        has_confirm,
        "disable should show a destructive InlineConfirm"
    );
    assert!(
        engine.config().enabled,
        "should remain enabled until confirmed"
    );
}

// @scenario: duress_mode :: Disable duress mode from settings
// @internal
#[test]
fn duress_confirm_disable_completes() {
    let mut engine = DuressPinEngine::new(enabled_config());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
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

// @internal
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

// @internal
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

// @internal
#[test]
fn duress_pin_accumulates_single_chars() {
    let mut engine = DuressPinEngine::new(default_config());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });

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

// @internal
#[test]
fn duress_pin_backspace_removes_last_char() {
    let mut engine = DuressPinEngine::new(default_config());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });

    for ch in ['1', '2', '3'] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: ch.to_string(),
        });
    }

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: String::new(),
    });

    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| {
            matches!(c, Component::PinInput { id, ..
        } if id == "pin")
        })
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

// The ConfigureAlerts step renders a picker over ALL contacts, marking
// the already-selected ones (2026-07-03-coercion-safety-config-gaps
// defect 1 — the wizard previously had no way to add a recipient).
// @scenario: duress_mode :: Configure trusted contacts for duress alerts
// @internal
#[test]
fn duress_alerts_renders_recipient_picker() {
    let mut engine = engine_at_alerts(config_with_contacts());
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "recipients".into(),
        item_id: "c1".into(),
    });

    let screen = engine.current_screen();
    let recipients = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == "recipients" => Some(items),
            _ => None,
        })
        .expect("a 'recipients' ToggleList");

    assert_eq!(recipients.len(), 2, "both contacts are offered");
    assert!(
        recipients.iter().any(|i| i.id == "c1" && i.selected),
        "c1 shows as selected"
    );
    assert!(
        recipients.iter().any(|i| i.id == "c2" && !i.selected),
        "c2 shows as unselected"
    );
}

// A completed duress setup must address at least one recipient — else the
// alert path (once wired) would send to nobody. The Save affordance is
// disabled AND pressing it is a no-op with zero recipients.
// @scenario: duress_mode :: Duress setup requires at least one alert recipient
// @internal
#[test]
fn duress_save_blocked_without_recipient() {
    let mut engine = engine_at_alerts(config_with_contacts());

    let save = engine
        .current_screen()
        .actions
        .into_iter()
        .find(|a| a.id == "save")
        .expect("a 'save' action");
    assert!(!save.enabled, "save is disabled with zero recipients");

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert_ne!(
        result,
        ActionResult::Complete,
        "save must not complete with zero recipients"
    );
    assert!(!engine.config().enabled, "config stays disabled");
}

// Selecting a recipient enables Save and round-trips the chosen id into
// the engine output (which persistence writes to alert_contact_ids).
// @internal
#[test]
fn duress_recipient_selection_round_trips_to_output() {
    let mut engine = engine_at_alerts(config_with_contacts());
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "recipients".into(),
        item_id: "c2".into(),
    });

    let save = engine
        .current_screen()
        .actions
        .into_iter()
        .find(|a| a.id == "save")
        .expect("a 'save' action");
    assert!(save.enabled, "save enabled once a recipient is chosen");

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert_eq!(result, ActionResult::Complete);

    match engine.engine_output() {
        Some(EngineOutput::DuressPin(setup)) => {
            assert_eq!(setup.alert_contact_ids, vec!["c2".to_string()]);
        }
        other => panic!("expected DuressPin output, got {other:?}"),
    }
}

// @internal
#[test]
fn duress_recipient_toggle_is_reversible() {
    let mut engine = engine_at_alerts(config_with_contacts());
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "recipients".into(),
        item_id: "c1".into(),
    });
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "recipients".into(),
        item_id: "c1".into(),
    });
    assert!(
        engine.config().selected_contact_ids.is_empty(),
        "toggling the same recipient twice deselects it"
    );
}
