//! Event System
//!
//! Callbacks for WebBook events.

use std::sync::Arc;

use crate::network::ConnectionState;
use crate::sync::SyncState;

/// Events emitted by WebBook.
#[derive(Debug, Clone)]
pub enum WebBookEvent {
    /// A contact was added.
    ContactAdded {
        /// The contact ID.
        contact_id: String,
    },

    /// A contact was updated.
    ContactUpdated {
        /// The contact ID.
        contact_id: String,
        /// Fields that changed.
        changed_fields: Vec<String>,
    },

    /// A contact was removed.
    ContactRemoved {
        /// The contact ID.
        contact_id: String,
    },

    /// Our own contact card was updated.
    OwnCardUpdated {
        /// Fields that changed.
        changed_fields: Vec<String>,
    },

    /// Sync state changed for a contact.
    SyncStateChanged {
        /// The contact ID.
        contact_id: String,
        /// The new sync state.
        state: SyncState,
    },

    /// Network connection state changed.
    ConnectionStateChanged {
        /// The new connection state.
        state: ConnectionState,
    },

    /// An incoming update was received from a contact.
    IncomingUpdate {
        /// The contact ID who sent the update.
        contact_id: String,
    },

    /// A message was successfully delivered.
    MessageDelivered {
        /// The contact ID the message was sent to.
        contact_id: String,
        /// The message ID.
        message_id: String,
    },

    /// A message delivery failed.
    MessageFailed {
        /// The contact ID the message was sent to.
        contact_id: String,
        /// Error description.
        error: String,
    },

    /// Error event for async operations.
    Error {
        /// Error description.
        message: String,
    },
}

/// Event handler trait.
///
/// Implement this trait to receive WebBook events.
pub trait EventHandler: Send + Sync {
    /// Called when an event occurs.
    fn on_event(&self, event: WebBookEvent);
}

/// Simple callback-based event handler.
///
/// Wraps a closure for easy event handling.
pub struct CallbackHandler<F>
where
    F: Fn(WebBookEvent) + Send + Sync,
{
    callback: F,
}

impl<F> CallbackHandler<F>
where
    F: Fn(WebBookEvent) + Send + Sync,
{
    /// Creates a new callback handler.
    pub fn new(callback: F) -> Self {
        CallbackHandler { callback }
    }
}

impl<F> EventHandler for CallbackHandler<F>
where
    F: Fn(WebBookEvent) + Send + Sync,
{
    fn on_event(&self, event: WebBookEvent) {
        (self.callback)(event);
    }
}

/// Event dispatcher for managing multiple handlers.
#[derive(Default)]
pub struct EventDispatcher {
    handlers: Vec<Arc<dyn EventHandler>>,
}

impl EventDispatcher {
    /// Creates a new event dispatcher.
    pub fn new() -> Self {
        EventDispatcher {
            handlers: Vec::new(),
        }
    }

    /// Adds an event handler.
    pub fn add_handler(&mut self, handler: Arc<dyn EventHandler>) {
        self.handlers.push(handler);
    }

    /// Removes all handlers.
    pub fn clear_handlers(&mut self) {
        self.handlers.clear();
    }

    /// Returns the number of registered handlers.
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    /// Dispatches an event to all handlers.
    pub fn dispatch(&self, event: WebBookEvent) {
        for handler in &self.handlers {
            handler.on_event(event.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_webbook_event_variants() {
        let event = WebBookEvent::ContactAdded {
            contact_id: "test-id".into(),
        };
        assert!(matches!(event, WebBookEvent::ContactAdded { .. }));

        let event = WebBookEvent::ContactUpdated {
            contact_id: "test-id".into(),
            changed_fields: vec!["email".into()],
        };
        assert!(matches!(event, WebBookEvent::ContactUpdated { .. }));
    }

    #[test]
    fn test_callback_handler() {
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();

        let handler = CallbackHandler::new(move |_event| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        handler.on_event(WebBookEvent::ContactAdded {
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

        dispatcher.dispatch(WebBookEvent::ContactAdded {
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

        dispatcher.dispatch(WebBookEvent::ContactAdded {
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
        let event = WebBookEvent::ContactUpdated {
            contact_id: "test".into(),
            changed_fields: vec!["email".into(), "phone".into()],
        };

        let cloned = event.clone();

        if let WebBookEvent::ContactUpdated { contact_id, changed_fields } = cloned {
            assert_eq!(contact_id, "test");
            assert_eq!(changed_fields.len(), 2);
        } else {
            panic!("Expected ContactUpdated event");
        }
    }

    #[test]
    fn test_sync_state_event() {
        let event = WebBookEvent::SyncStateChanged {
            contact_id: "test".into(),
            state: SyncState::Synced { last_sync: 12345 },
        };

        if let WebBookEvent::SyncStateChanged { contact_id, state } = event {
            assert_eq!(contact_id, "test");
            assert!(matches!(state, SyncState::Synced { .. }));
        } else {
            panic!("Expected SyncStateChanged event");
        }
    }

    #[test]
    fn test_connection_state_event() {
        let event = WebBookEvent::ConnectionStateChanged {
            state: ConnectionState::Connected,
        };

        if let WebBookEvent::ConnectionStateChanged { state } = event {
            assert_eq!(state, ConnectionState::Connected);
        } else {
            panic!("Expected ConnectionStateChanged event");
        }
    }
}
