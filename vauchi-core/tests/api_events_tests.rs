//! Tests for api::events
//! Extracted from events.rs

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
    let mut dispatcher = EventDispatcher::new();

    assert_eq!(dispatcher.handler_count(), 0);

    let handler = Arc::new(CallbackHandler::new(|_| {}));
    dispatcher.add_handler(handler);

    assert_eq!(dispatcher.handler_count(), 1);
}

#[test]
fn test_event_dispatcher_dispatch() {
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let mut dispatcher = EventDispatcher::new();

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

    let mut dispatcher = EventDispatcher::new();

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
    let mut dispatcher = EventDispatcher::new();

    let handler = Arc::new(CallbackHandler::new(|_| {}));
    dispatcher.add_handler(handler);

    assert_eq!(dispatcher.handler_count(), 1);

    dispatcher.clear_handlers();

    assert_eq!(dispatcher.handler_count(), 0);
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
