// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

fn sample_engine() -> SyncStatusEngine {
    SyncStatusEngine::new("wss://relay.vauchi.app".into(), 5, 2)
}

#[test]
fn sync_status_screen_id() {
    let engine = sample_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "sync_status");
}

#[test]
fn sync_status_title() {
    let engine = sample_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Sync");
}

#[test]
fn sync_status_shows_relay_url() {
    let engine = sample_engine();
    let screen = engine.current_screen();

    let has_relay_url = screen.components.iter().any(|c| match c {
        Component::InfoPanel { items, .. } => items
            .iter()
            .any(|item| item.detail == "wss://relay.vauchi.app"),
        _ => false,
    });
    assert!(has_relay_url, "Screen should display the relay URL");
}

#[test]
fn sync_status_sync_now_shows_alert() {
    let mut engine = sample_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Sync");
            assert!(!message.is_empty());
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

#[test]
fn sync_status_test_connection_shows_alert_with_url() {
    let mut engine = sample_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "test_connection".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Connection Test");
            assert!(
                message.contains("wss://relay.vauchi.app"),
                "Alert message should contain the relay URL, got: {message}"
            );
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

#[test]
fn sync_status_unknown_action_returns_update_screen() {
    let mut engine = sample_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "sync_status");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}
