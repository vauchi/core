// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for DebugSession — zero-cost debug instrumentation wrapper.

#![cfg(feature = "testing")]

use vauchi_core::diagnostic::debug_session::{DebugSession, ScreenId};
use vauchi_core::diagnostic::log_event::LogEventKind;

// ===== Inactive session (no-op) =====

// @internal
#[test]
fn inactive_session_is_default() {
    let session = DebugSession::new();
    assert!(!session.is_active());
}

// @internal
#[test]
fn inactive_session_ignores_events() {
    let mut session = DebugSession::new();
    session.log_screen_appeared(ScreenId::Home);
    session.log_screen_dismissed(ScreenId::Home);
    session.log_user_action(ScreenId::Home, "tap_exchange".to_string());

    // No events recorded when inactive
    assert!(session.events().is_empty());
}

// ===== Active session =====

// @internal
#[test]
fn activate_enables_event_collection() {
    let mut session = DebugSession::new();
    session.activate();
    assert!(session.is_active());

    session.log_screen_appeared(ScreenId::Home);
    // 2 events: DebugModeActivated + ScreenAppeared
    assert_eq!(session.events().len(), 2);
}

// @internal
#[test]
fn activation_logs_debug_mode_activated_event() {
    let mut session = DebugSession::new();
    session.activate();

    let events = session.events();
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, LogEventKind::DebugModeActivated));
}

// @internal
#[test]
fn deactivate_stops_collection() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_screen_appeared(ScreenId::Home);
    session.deactivate();
    session.log_screen_appeared(ScreenId::Settings);

    // Only the activation event + Home event, not Settings
    assert_eq!(session.events().len(), 2);
}

// ===== UX event logging =====

// @internal
#[test]
fn log_screen_appeared() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_screen_appeared(ScreenId::ExchangeStart);

    let events = session.events();
    // First event is DebugModeActivated, second is ScreenAppeared
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[1].kind,
        LogEventKind::ScreenAppeared { screen } if *screen == ScreenId::ExchangeStart
    ));
}

// @internal
#[test]
fn log_screen_dismissed() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_screen_dismissed(ScreenId::ExchangeQrScan);

    assert_eq!(session.events().len(), 2);
    assert!(matches!(
        &events_last(&session).kind,
        LogEventKind::ScreenDismissed { screen } if *screen == ScreenId::ExchangeQrScan
    ));
}

// @internal
#[test]
fn log_user_action() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_user_action(ScreenId::ContactList, "tap_contact".to_string());

    assert!(matches!(
        &events_last(&session).kind,
        LogEventKind::UserAction { screen, action }
            if *screen == ScreenId::ContactList && action == "tap_contact"
    ));
}

// @internal
#[test]
fn log_flow_abandoned() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_flow_abandoned(ScreenId::ExchangeQrDisplay, "user_backed_out".to_string());

    assert!(matches!(
        &events_last(&session).kind,
        LogEventKind::FlowAbandoned { screen, reason }
            if *screen == ScreenId::ExchangeQrDisplay && reason == "user_backed_out"
    ));
}

// @internal
#[test]
fn log_error_presented() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_error_presented(ScreenId::ExchangeFailure, "timeout".to_string());

    assert!(matches!(
        &events_last(&session).kind,
        LogEventKind::ErrorPresented { screen, error }
            if *screen == ScreenId::ExchangeFailure && error == "timeout"
    ));
}

// @internal
#[test]
fn log_tester_note() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_tester_note("QR hard to scan in bright sunlight".to_string());

    assert!(matches!(
        &events_last(&session).kind,
        LogEventKind::TesterNote { note }
            if note == "QR hard to scan in bright sunlight"
    ));
}

// @internal
#[test]
fn log_retry_attempted() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_retry_attempted(ScreenId::ExchangeQrScan, 2);

    assert!(matches!(
        &events_last(&session).kind,
        LogEventKind::RetryAttempted { screen, attempt }
            if *screen == ScreenId::ExchangeQrScan && *attempt == 2
    ));
}

// ===== ScreenId coverage =====

// @internal
#[test]
fn screen_id_serde_roundtrip() {
    let screens = vec![
        ScreenId::Onboarding,
        ScreenId::Home,
        ScreenId::ExchangeStart,
        ScreenId::ExchangeQrDisplay,
        ScreenId::ExchangeQrScan,
        ScreenId::ExchangeProximityVerification,
        ScreenId::ExchangeConfirmation,
        ScreenId::ExchangeSuccess,
        ScreenId::ExchangeFailure,
        ScreenId::ContactList,
        ScreenId::ContactDetail,
        ScreenId::SyncStatus,
        ScreenId::SyncConflictResolution,
        ScreenId::LinkDeviceStart,
        ScreenId::LinkDeviceQrDisplay,
        ScreenId::LinkDeviceQrScan,
        ScreenId::LinkDeviceConfirmation,
        ScreenId::LinkDeviceSuccess,
        ScreenId::Settings,
        ScreenId::DebugPanel,
    ];

    for screen in &screens {
        let json = serde_json::to_string(screen).expect("serialize");
        let deserialized: ScreenId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(*screen, deserialized);
    }
}

// ===== JSONL export =====

// @internal
#[test]
fn to_jsonl_exports_all_events() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_screen_appeared(ScreenId::Home);
    session.log_tester_note("test note".to_string());

    let jsonl = session.to_jsonl();
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), 3); // DebugModeActivated + ScreenAppeared + TesterNote

    for line in &lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("Invalid JSON: {}: {}", line, e));
        parsed.get("timestamp_ms").expect("expected Some");
        parsed.get("kind").expect("expected Some");
    }
}

// @internal
#[test]
fn inactive_session_jsonl_is_empty() {
    let session = DebugSession::new();
    assert!(session.to_jsonl().is_empty());
}

// ===== Session marker =====

// @internal
#[test]
fn log_session_marker() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_session_marker("phase_2_start".to_string());

    assert!(matches!(
        &events_last(&session).kind,
        LogEventKind::DebugSessionMarker { label }
            if label == "phase_2_start"
    ));
}

// ===== Re-activation semantics =====

// @internal
#[test]
fn activate_twice_is_idempotent_when_already_active() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_screen_appeared(ScreenId::Home);

    let events_before = session.events().len();
    session.activate(); // second activation

    // Re-activation should not clear existing events or add a duplicate activation event
    assert!(session.is_active());
    assert_eq!(session.events().len(), events_before);
}

// ===== Markdown export =====

// @internal
#[test]
fn to_markdown_empty_session() {
    let session = DebugSession::new();
    let md = session.to_markdown();
    assert!(md.contains("# Debug Session Report"));
    assert!(md.contains("inactive"));
    assert!(md.contains("0 events"));
    // Empty session should not contain a table
    assert!(!md.contains("|---:|---|"));
}

// @internal
#[test]
fn to_markdown_active_with_events() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_screen_appeared(ScreenId::Home);
    session.log_user_action(ScreenId::Home, "tap_exchange".to_string());

    let md = session.to_markdown();
    assert!(md.contains("# Debug Session Report"));
    assert!(md.contains("active"));
    assert!(md.contains("3 events")); // DebugModeActivated + ScreenAppeared + UserAction
    assert!(md.contains("| Timestamp"));
    assert!(md.contains("|---:|---|"));
    assert!(md.contains("DebugModeActivated"));
    assert!(md.contains("ScreenAppeared"));
    assert!(md.contains("UserAction"));
    // First event timestamp should be 0ms (or very close)
    assert!(md.contains("| 0 |") || md.contains("| 1 |"));
}

// @internal
#[test]
fn to_markdown_contains_screen_details() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_error_presented(ScreenId::ExchangeFailure, "timeout".to_string());

    let md = session.to_markdown();
    assert!(md.contains("ExchangeFailure"));
    assert!(md.contains("timeout"));
}

// @internal
#[test]
fn to_markdown_deactivated_session_shows_inactive_with_events() {
    let mut session = DebugSession::new();
    session.activate();
    session.log_screen_appeared(ScreenId::Home);
    session.deactivate();

    let md = session.to_markdown();
    assert!(
        md.contains("inactive"),
        "deactivated session must report inactive"
    );
    assert!(
        md.contains("2 events"),
        "events preserved after deactivation"
    );
}

// ===== Helpers =====

fn events_last(session: &DebugSession) -> &vauchi_core::diagnostic::log_event::LogEvent {
    session.events().last().expect("should have events")
}
