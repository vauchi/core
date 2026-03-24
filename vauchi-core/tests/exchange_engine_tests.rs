// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn make_engine() -> ExchangeEngine {
    ExchangeEngine::new(ExchangeConfig {
        own_name: "Alice".to_string(),
        own_qr_data: "alice-qr-payload".to_string(),
        available_groups: vec![],
    })
}

#[test]
fn exchange_starts_at_show_qr() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_show_qr");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 2);
    assert_eq!(screen.progress.as_ref().unwrap().total_steps, 6);
}

#[test]
fn exchange_shows_own_qr_data() {
    let engine = make_engine();
    let screen = engine.current_screen();

    assert_eq!(screen.components.len(), 1);
    match &screen.components[0] {
        Component::QrCode {
            id,
            data,
            mode,
            label,
        } => {
            assert_eq!(id, "own_qr");
            assert_eq!(data, "alice-qr-payload");
            assert_eq!(mode, &QrMode::Display);
            assert_eq!(label.as_deref(), Some("Alice"));
        }
        other => panic!("expected QrCode, got {:?}", other),
    }
}

#[test]
fn exchange_continue_to_scan() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });

    assert_eq!(result, ActionResult::RequestCamera);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_scan_qr");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 3);

    // Verify scan QR component
    match &screen.components[0] {
        Component::QrCode { data, mode, .. } => {
            assert_eq!(data, "");
            assert_eq!(mode, &QrMode::Scan);
        }
        other => panic!("expected QrCode in Scan mode, got {:?}", other),
    }
}

#[test]
fn exchange_scan_receives_data() {
    let mut engine = make_engine();
    // Move to scan step
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "exchange_verifying");
        }
        other => panic!("expected NavigateTo(verifying), got {:?}", other),
    }

    assert_eq!(engine.scanned_data(), Some("bob-qr-payload"));

    let screen = engine.current_screen();
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 4);
    assert!(screen.actions.is_empty());
}

#[test]
fn exchange_mark_success() {
    let mut engine = make_engine();
    // Move to scan → verifying
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });

    engine.mark_success();

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_success");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 5);

    match &screen.components[0] {
        Component::StatusIndicator { title, status, .. } => {
            assert_eq!(title, "Exchange Complete");
            assert_eq!(status, &Status::Success);
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }
}

#[test]
fn exchange_mark_failed() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });

    engine.mark_failed();

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_failed");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 6);

    match &screen.components[0] {
        Component::StatusIndicator { title, status, .. } => {
            assert_eq!(title, "Exchange Failed");
            assert_eq!(status, &Status::Failed);
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }

    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "retry");
    assert_eq!(screen.actions[0].style, ActionStyle::Primary);
    assert_eq!(screen.actions[1].id, "cancel");
    assert_eq!(screen.actions[1].style, ActionStyle::Secondary);
}

#[test]
fn exchange_success_done_completes() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });
    engine.mark_success();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".to_string(),
    });

    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn exchange_failed_retry_restarts() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });
    engine.mark_failed();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".to_string(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "exchange_show_qr");
        }
        other => panic!("expected NavigateTo(show_qr), got {:?}", other),
    }

    // Scanned data should be cleared on retry
    assert_eq!(engine.scanned_data(), None);
    assert_eq!(engine.current_screen().screen_id, "exchange_show_qr");
}

#[test]
fn exchange_back_from_scan() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".to_string(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "exchange_show_qr");
        }
        other => panic!("expected NavigateTo(show_qr), got {:?}", other),
    }

    assert_eq!(engine.current_screen().screen_id, "exchange_show_qr");
}

#[test]
fn exchange_failed_cancel_completes() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });
    engine.mark_failed();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".to_string(),
    });

    assert_eq!(result, ActionResult::Complete);
}
