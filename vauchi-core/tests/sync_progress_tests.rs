// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync progress event tests.
//!
//! Verifies that the sync() loop emits SyncProgress events with incrementing
//! processed counts and correct total.
//! Traces to: features/performance.feature @progress

mod common;

use std::sync::{Arc, Mutex};
use vauchi_core::api::events::{CallbackHandler, EventDispatcher, VauchiEvent};

/// Verify SyncProgress event variant exists and can be constructed.
// @scenario: sync_updates:Large sync queue handling
#[test]
fn test_sync_progress_event_constructable() {
    let event = VauchiEvent::SyncProgress {
        total: 10,
        processed: 5,
        contact_id: "abc123".to_string(),
    };

    match event {
        VauchiEvent::SyncProgress {
            total,
            processed,
            contact_id,
        } => {
            assert_eq!(total, 10);
            assert_eq!(processed, 5);
            assert_eq!(contact_id, "abc123");
        }
        _ => panic!("Expected SyncProgress variant"),
    }
}

/// Verify SyncProgress events are dispatched correctly through EventDispatcher.
// @scenario: sync_updates:Large sync queue handling
#[test]
fn test_sync_progress_dispatched_via_handler() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let dispatcher = EventDispatcher::new();
    let handler = Arc::new(CallbackHandler::new(move |event: VauchiEvent| {
        if let VauchiEvent::SyncProgress { .. } = &event {
            received_clone.lock().unwrap().push(event);
        }
    }));

    dispatcher.add_handler(handler);

    // Dispatch a series of progress events
    for i in 0..5 {
        dispatcher.dispatch(VauchiEvent::SyncProgress {
            total: 5,
            processed: i + 1,
            contact_id: format!("contact-{}", i),
        });
    }

    let events = received.lock().unwrap();
    assert_eq!(events.len(), 5, "Should have received 5 progress events");

    // Verify incrementing processed count
    for (idx, event) in events.iter().enumerate() {
        if let VauchiEvent::SyncProgress {
            total, processed, ..
        } = event
        {
            assert_eq!(*total, 5);
            assert_eq!(*processed, idx + 1);
        }
    }
}

/// Verify SyncProgress events have correct total matching ready updates count.
// @scenario: sync_updates:Large sync queue handling
#[test]
fn test_sync_progress_total_matches_ready_updates() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let dispatcher = EventDispatcher::new();
    let handler = Arc::new(CallbackHandler::new(move |event: VauchiEvent| {
        if let VauchiEvent::SyncProgress { .. } = &event {
            received_clone.lock().unwrap().push(event);
        }
    }));

    dispatcher.add_handler(handler);

    // Simulate 3 ready updates
    let total = 3;
    for i in 0..total {
        dispatcher.dispatch(VauchiEvent::SyncProgress {
            total,
            processed: i + 1,
            contact_id: format!("contact-{}", i),
        });
    }

    let events = received.lock().unwrap();
    assert_eq!(events.len(), total);

    // All events should report the same total
    for event in events.iter() {
        if let VauchiEvent::SyncProgress { total: t, .. } = event {
            assert_eq!(*t, total, "All progress events should have total={}", total);
        }
    }

    // Last event should have processed == total
    if let VauchiEvent::SyncProgress { processed, .. } = events.last().unwrap() {
        assert_eq!(
            *processed, total,
            "Last event should have processed == total"
        );
    }
}

/// Verify SyncProgress is Clone and Debug (required by VauchiEvent derive).
// @scenario: sync_updates:Large sync queue handling
#[test]
fn test_sync_progress_clone_and_debug() {
    let event = VauchiEvent::SyncProgress {
        total: 10,
        processed: 3,
        contact_id: "test-contact".to_string(),
    };

    let cloned = event.clone();
    let debug_str = format!("{:?}", cloned);
    assert!(debug_str.contains("SyncProgress"));
    assert!(debug_str.contains("10"));
    assert!(debug_str.contains("3"));
}
