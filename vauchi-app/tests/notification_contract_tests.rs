// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contract tests for notification types (CC-05).
//!
//! Verifies serde roundtrip stability for all public types crossing the
//! core/frontend boundary: ActivityLogEntry, NotificationPreferences, EventOrigin.

use vauchi_app::notification_types::{ActivityLogEntry, EventOrigin, NotificationPreferences};

// @scenario: notification-types.feature - ActivityLogEntry serializes and deserializes identically
#[test]
fn activity_log_entry_serde_roundtrip() {
    let entries = vec![
        ActivityLogEntry::CardUpdateReceived {
            contact_id: "abc".into(),
            changed_fields: vec!["phone".into(), "email".into()],
        },
        ActivityLogEntry::CardUpdateDelivered {
            contact_id: "def".into(),
        },
        ActivityLogEntry::CardUpdatePending {
            contact_id: "ghi".into(),
        },
        ActivityLogEntry::CardUpdateFailed {
            contact_id: "jkl".into(),
            reason: "timeout".into(),
        },
        ActivityLogEntry::ContactAdded {
            contact_id: "mno".into(),
            origin: EventOrigin::Synced,
        },
        ActivityLogEntry::EmergencyAlertReceived {
            contact_id: "pqr".into(),
        },
    ];

    for entry in entries {
        let json = serde_json::to_string(&entry).unwrap();
        let decoded: ActivityLogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, decoded, "roundtrip failed for {json}");
    }
}

// @scenario: notification-types.feature - NotificationPreferences serializes and deserializes identically
#[test]
fn notification_preferences_roundtrip() {
    for enabled in [true, false] {
        let prefs = NotificationPreferences {
            contact_added_enabled: enabled,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let decoded: NotificationPreferences = serde_json::from_str(&json).unwrap();
        assert_eq!(prefs.contact_added_enabled, decoded.contact_added_enabled);
    }
}

// @scenario: notification-types.feature - EventOrigin serializes and deserializes identically
#[test]
fn event_origin_roundtrip() {
    for origin in [EventOrigin::Local, EventOrigin::Synced] {
        let json = serde_json::to_string(&origin).unwrap();
        let decoded: EventOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(origin, decoded);
    }
}
