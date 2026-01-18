//! Event System
//!
//! Callbacks for Vauchi events.

use std::sync::Arc;

use crate::network::ConnectionState;
use crate::sync::SyncState;

/// Events emitted by Vauchi.
#[derive(Debug, Clone)]
pub enum VauchiEvent {
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
/// Implement this trait to receive Vauchi events.
pub trait EventHandler: Send + Sync {
    /// Called when an event occurs.
    fn on_event(&self, event: VauchiEvent);
}

/// Simple callback-based event handler.
///
/// Wraps a closure for easy event handling.
pub struct CallbackHandler<F>
where
    F: Fn(VauchiEvent) + Send + Sync,
{
    callback: F,
}

impl<F> CallbackHandler<F>
where
    F: Fn(VauchiEvent) + Send + Sync,
{
    /// Creates a new callback handler.
    pub fn new(callback: F) -> Self {
        CallbackHandler { callback }
    }
}

impl<F> EventHandler for CallbackHandler<F>
where
    F: Fn(VauchiEvent) + Send + Sync,
{
    fn on_event(&self, event: VauchiEvent) {
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
    pub fn dispatch(&self, event: VauchiEvent) {
        for handler in &self.handlers {
            handler.on_event(event.clone());
        }
    }
}
