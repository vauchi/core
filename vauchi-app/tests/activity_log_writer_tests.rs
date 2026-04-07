// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ActivityLogWriter.
//!
//! Covers event-to-entry mapping, deduplication via INSERT OR IGNORE, and
//! that ignored event variants produce no log entries.

use vauchi_app::activity_log_writer::ActivityLogWriter;
use vauchi_app::notification_types::{ActivityLogEntry, EventOrigin};
use vauchi_core::Storage;
use vauchi_core::VauchiEvent;
use vauchi_core::crypto::SymmetricKey;

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).expect("in-memory storage")
}

const NOW: u64 = 1_700_000_000;

// @scenario: activity-log.feature - ContactAdded event creates a log entry
// @internal
#[test]
fn contact_added_event_creates_log_entry() {
    let storage = test_storage();
    let contact_id = "contact-abc".to_owned();

    let events = vec![VauchiEvent::contact_added(
        contact_id.clone(),
        EventOrigin::Synced,
    )];

    let result = ActivityLogWriter::write(&storage, &events, NOW).unwrap();

    assert_eq!(result.len(), 1, "exactly one entry should be inserted");

    let (event_key, entry) = &result[0];
    assert_eq!(event_key, &format!("contact_added:{contact_id}"));

    match entry {
        ActivityLogEntry::ContactAdded {
            contact_id: cid,
            origin,
        } => {
            assert_eq!(cid, &contact_id);
            assert_eq!(*origin, EventOrigin::Synced);
        }
        other => panic!("unexpected entry variant: {other:?}"),
    }

    // Verify the row is queryable from storage.
    let rows = storage
        .activity_log_query_recent(NOW, 7 * 24 * 3600)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_key, format!("contact_added:{contact_id}"));
    assert_eq!(rows[0].category, "contact_added");
}

// @scenario: activity-log.feature - Duplicate event key is silently skipped
// @internal
#[test]
fn duplicate_event_returns_empty() {
    let storage = test_storage();
    let contact_id = "contact-dup".to_owned();

    let events = vec![VauchiEvent::contact_added(
        contact_id.clone(),
        EventOrigin::Local,
    )];

    // First write — should insert.
    let first = ActivityLogWriter::write(&storage, &events, NOW).unwrap();
    assert_eq!(first.len(), 1, "first write should insert one entry");

    // Second write with the same events — event_key is identical, should be skipped.
    let second = ActivityLogWriter::write(&storage, &events, NOW).unwrap();
    assert_eq!(second.len(), 0, "duplicate write must return empty vec");

    // Only one row should exist in storage.
    let rows = storage
        .activity_log_query_recent(NOW, 7 * 24 * 3600)
        .unwrap();
    assert_eq!(rows.len(), 1, "storage must contain only one row");
}

// @scenario: activity-log.feature - EmergencyAlertReceived creates a log entry
// @internal
#[test]
fn emergency_alert_creates_log_entry() {
    let storage = test_storage();
    let contact_id = "contact-emergency".to_owned();

    let events = vec![VauchiEvent::EmergencyAlertReceived {
        contact_id: contact_id.clone(),
        message: "SOS".to_owned(),
        timestamp: NOW,
        location: Some((48.8584, 2.2945)),
    }];

    let result = ActivityLogWriter::write(&storage, &events, NOW).unwrap();

    assert_eq!(result.len(), 1, "exactly one entry should be inserted");

    let (event_key, entry) = &result[0];
    assert_eq!(event_key, &format!("emergency:{contact_id}:{NOW}"));

    match entry {
        ActivityLogEntry::EmergencyAlertReceived { contact_id: cid } => {
            assert_eq!(cid, &contact_id);
        }
        other => panic!("unexpected entry variant: {other:?}"),
    }

    let rows = storage
        .activity_log_query_recent(NOW, 7 * 24 * 3600)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].category, "emergency_alert_received");
}

// @scenario: activity-log.feature - Unrelated events are ignored
// @internal
#[test]
fn unrelated_events_produce_no_entries() {
    let storage = test_storage();

    let events = vec![
        VauchiEvent::ContactUpdated {
            contact_id: "c1".to_owned(),
            changed_fields: vec!["name".to_owned()],
        },
        VauchiEvent::OwnCardUpdated {
            changed_fields: vec!["phone".to_owned()],
        },
        VauchiEvent::ContactRemoved {
            contact_id: "c2".to_owned(),
        },
    ];

    let result = ActivityLogWriter::write(&storage, &events, NOW).unwrap();
    assert_eq!(result.len(), 0, "no entries expected for unrelated events");

    let rows = storage
        .activity_log_query_recent(NOW, 7 * 24 * 3600)
        .unwrap();
    assert_eq!(rows.len(), 0);
}

// @scenario: activity-log.feature - IncomingUpdate creates a card_received entry
// @internal
#[test]
fn incoming_update_creates_card_received_entry() {
    let storage = test_storage();
    let contact_id = "contact-incoming".to_owned();

    let events = vec![VauchiEvent::IncomingUpdate {
        contact_id: contact_id.clone(),
    }];

    let result = ActivityLogWriter::write(&storage, &events, NOW).unwrap();

    assert_eq!(result.len(), 1);
    let (event_key, entry) = &result[0];
    assert_eq!(event_key, &format!("card_received:{contact_id}:{NOW}"));

    match entry {
        ActivityLogEntry::CardUpdateReceived {
            contact_id: cid,
            changed_fields,
        } => {
            assert_eq!(cid, &contact_id);
            assert!(
                changed_fields.is_empty(),
                "changed_fields must be empty vec"
            );
        }
        other => panic!("unexpected entry variant: {other:?}"),
    }
}

// @scenario: activity-log.feature - MessageDelivered creates a card_delivered entry
// @internal
#[test]
fn message_delivered_creates_card_delivered_entry() {
    let storage = test_storage();
    let contact_id = "contact-delivered".to_owned();
    let message_id = "msg-001".to_owned();

    let events = vec![VauchiEvent::MessageDelivered {
        contact_id: contact_id.clone(),
        message_id: message_id.clone(),
    }];

    let result = ActivityLogWriter::write(&storage, &events, NOW).unwrap();

    assert_eq!(result.len(), 1);
    let (event_key, entry) = &result[0];
    assert_eq!(
        event_key,
        &format!("card_delivered:{contact_id}:{message_id}")
    );

    match entry {
        ActivityLogEntry::CardUpdateDelivered { contact_id: cid } => {
            assert_eq!(cid, &contact_id);
        }
        other => panic!("unexpected entry variant: {other:?}"),
    }
}

// @scenario: activity-log.feature - MessageFailed creates a card_failed entry
// @internal
#[test]
fn message_failed_creates_card_failed_entry() {
    let storage = test_storage();
    let contact_id = "contact-failed".to_owned();
    let error = "timeout".to_owned();

    let events = vec![VauchiEvent::MessageFailed {
        contact_id: contact_id.clone(),
        error: error.clone(),
    }];

    let result = ActivityLogWriter::write(&storage, &events, NOW).unwrap();

    assert_eq!(result.len(), 1);
    let (event_key, entry) = &result[0];
    assert_eq!(event_key, &format!("card_failed:{contact_id}:{NOW}"));

    match entry {
        ActivityLogEntry::CardUpdateFailed {
            contact_id: cid,
            reason,
        } => {
            assert_eq!(cid, &contact_id);
            assert_eq!(reason, &error);
        }
        other => panic!("unexpected entry variant: {other:?}"),
    }
}
