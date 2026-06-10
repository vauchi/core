// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the sync status workflow engine.
//!
//! Verifies that sync_now and test_connection actions return the correct
//! results and that connection state is displayed accurately.

use vauchi_app::ui::{
    ActionResult, EngineOutput, SyncChoice, SyncStatusEngine, UserAction, WorkflowEngine,
};
use vauchi_core::network::ConnectionState;

// @scenario: sync.feature - Sync screen shows relay info and actions
#[test]
fn test_sync_screen_shows_relay_and_actions() {
    let engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 42, 3);
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "sync_status");
    assert_eq!(screen.title, "Sync");
    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "sync_now");
    assert_eq!(screen.actions[1].id, "test_connection");

    let screen_json = serde_json::to_string(&screen).unwrap();
    assert!(
        screen_json.contains("relay.vauchi.app"),
        "Screen must show relay URL"
    );
    assert!(screen_json.contains("42"), "Screen must show contact count");
    assert!(
        screen_json.contains("3 update(s) waiting to sync"),
        "Screen must show pending update count"
    );
}

// @scenario: sync.feature - Sync now returns Complete with collected input
#[test]
fn test_sync_now_returns_complete_with_input() {
    let mut engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 5, 0);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });

    assert!(
        matches!(result, ActionResult::Complete),
        "sync_now should return Complete for routing"
    );
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::Sync(SyncChoice::SyncNow)),
        "engine_output must signal sync_now to the router"
    );
}

// @scenario: sync.feature - Test connection returns Complete with collected input
#[test]
fn test_connection_returns_complete_with_input() {
    let mut engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 5, 0);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "test_connection".into(),
    });

    assert!(
        matches!(result, ActionResult::Complete),
        "test_connection should return Complete for routing"
    );
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::Sync(SyncChoice::TestConnection)),
        "engine_output must signal test_connection to the router"
    );
}

// @scenario: sync.feature - Offline state disables sync button
#[test]
fn test_offline_disables_sync_button() {
    let engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 0, 0)
        .with_connection_state(ConnectionState::Disconnected);
    let screen = engine.current_screen();

    let sync_action = screen.actions.iter().find(|a| a.id == "sync_now").unwrap();
    assert!(
        !sync_action.enabled,
        "Sync Now must be disabled when offline"
    );

    let test_action = screen
        .actions
        .iter()
        .find(|a| a.id == "test_connection")
        .unwrap();
    assert!(
        test_action.enabled,
        "Test Connection must remain enabled when offline"
    );
}

// @scenario: sync.feature - Connected state shows success indicator
#[test]
fn test_connected_state_shows_success() {
    let engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 10, 0)
        .with_connection_state(ConnectionState::Connected);
    let screen = engine.current_screen();

    let screen_json = serde_json::to_string(&screen).unwrap();
    assert!(
        screen_json.contains("Connected"),
        "Screen must show Connected status"
    );
}

// @scenario: sync.feature - Zero pending shows all up to date
#[test]
fn test_zero_pending_shows_up_to_date() {
    let engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 10, 0);
    let screen = engine.current_screen();

    let screen_json = serde_json::to_string(&screen).unwrap();
    assert!(
        screen_json.contains("All up to date"),
        "Zero pending must show 'All up to date'"
    );
}
