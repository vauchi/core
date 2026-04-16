// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for AppEngine activity log wiring.

use std::time::{SystemTime, UNIX_EPOCH};
use vauchi_app::ui::AppEngine;
use vauchi_core::Vauchi;
use vauchi_core::api::EventOrigin;

// @internal
#[test]
fn app_engine_writes_events_to_activity_log() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();

    let mut engine = AppEngine::new(vauchi);

    // Dispatch an event manually via the inner vauchi instance
    engine
        .vauchi()
        .dispatch_event(vauchi_core::VauchiEvent::contact_added(
            "contact-123".to_string(),
            EventOrigin::Local,
        ));

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // The event should be in the channel but not yet in the log.
    let rows = engine
        .vauchi()
        .storage()
        .activity_log_query_recent(now, 7 * 86400)
        .unwrap();
    assert_eq!(rows.len(), 0, "Log should be empty before drain");

    // Call poll_notifications which triggers drain
    let _ = engine.poll_notifications();

    // Now it should be in the log
    let rows = engine
        .vauchi()
        .storage()
        .activity_log_query_recent(now, 7 * 86400)
        .unwrap();
    assert_eq!(rows.len(), 1, "Log should have 1 entry after drain");
    assert_eq!(rows[0].category, "contact_added");
    assert!(rows[0].event_key.contains("contact-123"));
}

// @internal
#[test]
fn app_engine_shows_notifications_from_log() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();

    let mut engine = AppEngine::new(vauchi);

    // Dispatch an emergency alert
    engine
        .vauchi()
        .dispatch_event(vauchi_core::VauchiEvent::EmergencyAlertReceived {
            contact_id: "danger-contact".to_string(),
            message: "HELP".to_string(),
            timestamp: 1700000000,
            location: None,
        });

    // Poll notifications
    let notifications = engine.poll_notifications();

    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].title, "Emergency Alert");
}
