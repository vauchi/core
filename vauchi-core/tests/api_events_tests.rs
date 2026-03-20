// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for api::events
//! Extracted from events.rs

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use vauchi_core::api::*;
use vauchi_core::*;

#[test]
fn test_vauchi_event_variants() {
    let event = VauchiEvent::ContactAdded {
        contact_id: "test-id".into(),
    };
    assert!(matches!(event, VauchiEvent::ContactAdded { .. }));

    let event = VauchiEvent::ContactUpdated {
        contact_id: "test-id".into(),
        changed_fields: vec!["email".into()],
    };
    assert!(matches!(event, VauchiEvent::ContactUpdated { .. }));
}

#[test]
fn test_callback_handler() {
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let handler = CallbackHandler::new(move |_event| {
        count_clone.fetch_add(1, Ordering::SeqCst);
    });

    handler.on_event(VauchiEvent::ContactAdded {
        contact_id: "test".into(),
    });

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_event_dispatcher_add_handler() {
    let dispatcher = EventDispatcher::new();

    assert_eq!(dispatcher.handler_count(), 0);

    let handler = Arc::new(CallbackHandler::new(|_| {}));
    let id = dispatcher.add_handler(handler);

    assert_eq!(dispatcher.handler_count(), 1);
    assert!(id > 0, "HandlerId should be positive");
}

#[test]
fn test_event_dispatcher_dispatch() {
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let dispatcher = EventDispatcher::new();

    let handler = Arc::new(CallbackHandler::new(move |_| {
        count_clone.fetch_add(1, Ordering::SeqCst);
    }));

    dispatcher.add_handler(handler);

    dispatcher.dispatch(VauchiEvent::ContactAdded {
        contact_id: "test".into(),
    });

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_event_dispatcher_multiple_handlers() {
    let count = Arc::new(AtomicUsize::new(0));

    let dispatcher = EventDispatcher::new();

    // Add 3 handlers
    for _ in 0..3 {
        let count_clone = count.clone();
        let handler = Arc::new(CallbackHandler::new(move |_| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        }));
        dispatcher.add_handler(handler);
    }

    dispatcher.dispatch(VauchiEvent::ContactAdded {
        contact_id: "test".into(),
    });

    // All 3 handlers should be called
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
fn test_event_dispatcher_clear_handlers() {
    let dispatcher = EventDispatcher::new();

    let handler = Arc::new(CallbackHandler::new(|_| {}));
    dispatcher.add_handler(handler);

    assert_eq!(dispatcher.handler_count(), 1);

    dispatcher.clear_handlers();

    assert_eq!(dispatcher.handler_count(), 0);
}

/// Test: remove_handler removes a specific handler by ID (#89).
#[test]
fn test_event_dispatcher_remove_handler() {
    let count_a = Arc::new(AtomicUsize::new(0));
    let count_b = Arc::new(AtomicUsize::new(0));

    let dispatcher = EventDispatcher::new();

    let count_a_clone = count_a.clone();
    let handler_a = Arc::new(CallbackHandler::new(move |_| {
        count_a_clone.fetch_add(1, Ordering::SeqCst);
    }));
    let id_a = dispatcher.add_handler(handler_a);

    let count_b_clone = count_b.clone();
    let handler_b = Arc::new(CallbackHandler::new(move |_| {
        count_b_clone.fetch_add(1, Ordering::SeqCst);
    }));
    dispatcher.add_handler(handler_b);

    // Both fire on dispatch
    dispatcher.dispatch(VauchiEvent::ContactAdded {
        contact_id: "test".into(),
    });
    assert_eq!(count_a.load(Ordering::SeqCst), 1);
    assert_eq!(count_b.load(Ordering::SeqCst), 1);

    // Remove handler A
    assert!(
        dispatcher.remove_handler(id_a),
        "Should find and remove handler A"
    );
    assert_eq!(dispatcher.handler_count(), 1);

    // Only handler B fires now
    dispatcher.dispatch(VauchiEvent::ContactAdded {
        contact_id: "test2".into(),
    });
    assert_eq!(
        count_a.load(Ordering::SeqCst),
        1,
        "Handler A should not fire after removal"
    );
    assert_eq!(
        count_b.load(Ordering::SeqCst),
        2,
        "Handler B should still fire"
    );

    // Removing unknown ID returns false
    assert!(
        !dispatcher.remove_handler(999),
        "Unknown ID should return false"
    );
}

#[test]
fn test_event_clone() {
    let event = VauchiEvent::ContactUpdated {
        contact_id: "test".into(),
        changed_fields: vec!["email".into(), "phone".into()],
    };

    let cloned = event.clone();

    if let VauchiEvent::ContactUpdated {
        contact_id,
        changed_fields,
    } = cloned
    {
        assert_eq!(contact_id, "test");
        assert_eq!(changed_fields.len(), 2);
    } else {
        panic!("Expected ContactUpdated event");
    }
}

#[test]
fn test_sync_state_event() {
    let event = VauchiEvent::SyncStateChanged {
        contact_id: "test".into(),
        state: SyncState::Synced { last_sync: 12345 },
    };

    if let VauchiEvent::SyncStateChanged { contact_id, state } = event {
        assert_eq!(contact_id, "test");
        assert!(matches!(state, SyncState::Synced { .. }));
    } else {
        panic!("Expected SyncStateChanged event");
    }
}

#[test]
fn test_connection_state_event() {
    let event = VauchiEvent::ConnectionStateChanged {
        state: ConnectionState::Connected,
    };

    if let VauchiEvent::ConnectionStateChanged { state } = event {
        assert_eq!(state, ConnectionState::Connected);
    } else {
        panic!("Expected ConnectionStateChanged event");
    }
}
