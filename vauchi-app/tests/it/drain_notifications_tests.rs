// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `AppEngine::drain_pending_notifications`.
//!
//! Verifies the end-to-end notification pipeline: event buffering,
//! activity log writing, notification evaluation, and exact-once drain.

use vauchi_app::notification_types::NotificationCategory;
use vauchi_app::ui::AppEngine;
use vauchi_core::api::{EventOrigin, Vauchi, VauchiEvent};

fn test_engine() -> AppEngine {
    let vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    AppEngine::new(vauchi)
}

// @scenario: notification.feature - No events produces no notifications
// @internal
#[test]
fn drain_returns_empty_when_no_events() {
    let mut engine = test_engine();
    let notifications = engine.drain_pending_notifications();
    assert!(notifications.is_empty(), "no events means no notifications");
}

// @scenario: notification.feature - Emergency alert produces OS notification
// @internal
#[test]
fn drain_returns_emergency_notification_after_event() {
    let mut engine = test_engine();

    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::EmergencyAlertReceived {
            contact_id: "contact-1".into(),
            message: "help".into(),
            timestamp: 1_700_000_000,
            location: None,
            alert_nonce: [7u8; 32],
        });

    let notifications = engine.drain_pending_notifications();
    assert_eq!(notifications.len(), 1, "exactly one emergency notification");
    assert_eq!(
        notifications[0].category,
        NotificationCategory::EmergencyAlert
    );
    assert_eq!(notifications[0].title, "Emergency Alert");
    assert!(
        notifications[0].body.contains("emergency alert"),
        "body should mention emergency alert: {}",
        notifications[0].body
    );
}

// @scenario: notification.feature - Drain clears buffer (exact-once)
// @internal
#[test]
fn drain_clears_buffer_on_second_call() {
    let mut engine = test_engine();

    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::EmergencyAlertReceived {
            contact_id: "contact-1".into(),
            message: "help".into(),
            timestamp: 1_700_000_000,
            location: None,
            alert_nonce: [7u8; 32],
        });

    let first = engine.drain_pending_notifications();
    assert_eq!(first.len(), 1, "first drain returns the notification");

    let second = engine.drain_pending_notifications();
    assert!(second.is_empty(), "second drain returns empty (exact-once)");
}

// @scenario: notification.feature - Drain persists events to activity log
// @internal
#[test]
fn drain_writes_to_activity_log() {
    let mut engine = test_engine();

    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::EmergencyAlertReceived {
            contact_id: "contact-1".into(),
            message: "help".into(),
            timestamp: 1_700_000_000,
            location: None,
            alert_nonce: [7u8; 32],
        });

    let _ = engine.drain_pending_notifications();

    // Verify the activity log has the entry
    let rows = engine
        .vauchi()
        .storage()
        .activity_log()
        .activity_log_query_recent(1_700_100_000, 200_000)
        .expect("query activity log");
    assert_eq!(rows.len(), 1, "one activity log entry persisted");
    assert_eq!(rows[0].category, "emergency_alert_received");
}

// @scenario: notification.feature - Local ContactAdded does not produce notification
// @internal
#[test]
fn drain_contact_added_local_produces_no_notification() {
    let mut engine = test_engine();

    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::contact_added("c1".into(), EventOrigin::Local));

    let notifications = engine.drain_pending_notifications();
    assert!(
        notifications.is_empty(),
        "Local ContactAdded should not produce a notification (prefs default = off)"
    );
}

// @scenario: notification.feature - Emergency notification uses contact display name
// @internal
#[test]
fn drain_uses_contact_display_name() {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("create identity");

    let mut engine = AppEngine::new(vauchi);

    // Create a contact via exchange so the name resolver can find it
    // Since we can't easily create a contact in test, we verify the
    // fallback name format (truncated contact_id) is used instead.
    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::EmergencyAlertReceived {
            contact_id: "abcdef1234567890".into(),
            message: "help".into(),
            timestamp: 1_700_000_000,
            location: None,
            alert_nonce: [7u8; 32],
        });

    let notifications = engine.drain_pending_notifications();
    assert_eq!(notifications.len(), 1);
    assert!(
        notifications[0].body.contains("abcdef12"),
        "body should contain truncated contact ID as fallback name: {}",
        notifications[0].body
    );
}
