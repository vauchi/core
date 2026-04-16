// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;
use vauchi_core::network::ConnectionState;

fn connected_engine() -> SyncStatusEngine {
    SyncStatusEngine::new("https://relay.vauchi.app".into(), 5, 2)
        .with_connection_state(ConnectionState::Connected)
}

fn offline_engine() -> SyncStatusEngine {
    SyncStatusEngine::new("https://relay.vauchi.app".into(), 5, 3)
        .with_connection_state(ConnectionState::Disconnected)
}

// @internal
#[test]
fn sync_status_screen_id() {
    let engine = connected_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "sync_status");
}

// @internal
#[test]
fn sync_status_title() {
    let engine = connected_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Sync");
}

// @internal
#[test]
fn sync_status_shows_relay_url() {
    let engine = connected_engine();
    let screen = engine.current_screen();

    let has_relay_url = screen.components.iter().any(|c| match c {
        Component::InfoPanel { items, .. } => items
            .iter()
            .any(|item| item.detail == "https://relay.vauchi.app"),
        _ => false,
    });
    assert!(has_relay_url, "Screen should display the relay URL");
}

// @internal
#[test]
fn connected_shows_success_status() {
    let engine = connected_engine();
    let screen = engine.current_screen();

    let status = screen.components.iter().find_map(|c| match c {
        Component::StatusIndicator { title, status, .. } => Some((title.as_str(), status.clone())),
        _ => None,
    });
    let (title, st) = status.expect("Should have StatusIndicator");
    assert!(
        title.contains("Connected"),
        "Connected engine should show 'Connected', got: {title}"
    );
    assert_eq!(st, Status::Success);
}

// @internal
#[test]
fn offline_shows_failed_status_with_guidance() {
    let engine = offline_engine();
    let screen = engine.current_screen();

    let indicator = screen.components.iter().find_map(|c| match c {
        Component::StatusIndicator {
            title,
            detail,
            status,
            ..
        } => Some((title.clone(), detail.clone(), status.clone())),
        _ => None,
    });
    let (title, detail, st) = indicator.expect("Should have StatusIndicator");
    assert!(title.contains("Offline"), "Title: {title}");
    assert_eq!(st, Status::Failed);
    assert!(
        detail
            .as_deref()
            .unwrap_or("")
            .contains("will sync automatically"),
        "Should show guidance when offline: {detail:?}"
    );
}

// @internal
#[test]
fn offline_disables_sync_now_button() {
    let engine = offline_engine();
    let screen = engine.current_screen();

    let sync_now = screen.actions.iter().find(|a| a.id == "sync_now").unwrap();
    assert!(
        !sync_now.enabled,
        "Sync Now should be disabled when offline"
    );
}

// @internal
#[test]
fn offline_shows_retry_connection_label() {
    let engine = offline_engine();
    let screen = engine.current_screen();

    let test_btn = screen
        .actions
        .iter()
        .find(|a| a.id == "test_connection")
        .unwrap();
    assert_eq!(
        test_btn.label, "Retry Connection",
        "Button should say 'Retry Connection' when offline"
    );
}

// @internal
#[test]
fn connected_shows_test_connection_label() {
    let engine = connected_engine();
    let screen = engine.current_screen();

    let test_btn = screen
        .actions
        .iter()
        .find(|a| a.id == "test_connection")
        .unwrap();
    assert_eq!(
        test_btn.label, "Test Connection",
        "Button should say 'Test Connection' when connected"
    );
}

// @internal
#[test]
fn sync_now_returns_complete() {
    let mut engine = connected_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "sync_now should return Complete for AppEngine to handle"
    );
    assert_eq!(engine.collected_input().as_deref(), Some("sync_now"));
}

// @internal
#[test]
fn test_connection_returns_complete() {
    let mut engine = offline_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "test_connection".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "test_connection should return Complete for AppEngine to handle"
    );
    assert_eq!(engine.collected_input().as_deref(), Some("test_connection"));
}

// @internal
#[test]
fn reconnecting_shows_in_progress() {
    let engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 5, 0)
        .with_connection_state(ConnectionState::Reconnecting { attempt: 2 });
    let screen = engine.current_screen();

    let status = screen.components.iter().find_map(|c| match c {
        Component::StatusIndicator { status, title, .. } => Some((title.clone(), status.clone())),
        _ => None,
    });
    let (title, st) = status.expect("Should have StatusIndicator");
    assert!(title.contains("Reconnecting"), "Title: {title}");
    assert_eq!(st, Status::InProgress);
}

// @internal
#[test]
fn zero_pending_shows_all_up_to_date() {
    let engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 3, 0)
        .with_connection_state(ConnectionState::Connected);
    let screen = engine.current_screen();

    let pending = screen.components.iter().find_map(|c| match c {
        Component::InfoPanel { items, .. } => items
            .iter()
            .find(|i| i.title == "Pending Updates")
            .map(|i| i.detail.clone()),
        _ => None,
    });
    assert_eq!(
        pending.as_deref(),
        Some("All up to date"),
        "Zero pending should show 'All up to date'"
    );
}

// @internal
#[test]
fn nonzero_pending_shows_count() {
    let engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 3, 7)
        .with_connection_state(ConnectionState::Disconnected);
    let screen = engine.current_screen();

    let pending = screen.components.iter().find_map(|c| match c {
        Component::InfoPanel { items, .. } => items
            .iter()
            .find(|i| i.title == "Pending Updates")
            .map(|i| i.detail.clone()),
        _ => None,
    });
    assert!(
        pending.as_deref().unwrap_or("").contains("7 update(s)"),
        "Should show pending count: {pending:?}"
    );
}

// @internal
#[test]
fn unknown_action_returns_update_screen() {
    let mut engine = connected_engine();
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
