// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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

    /// Delivery status update for a specific message.
    DeliveryStatusUpdate {
        /// The message ID.
        message_id: String,
        /// The new delivery status.
        status: String,
    },

    /// Warning that a message is about to expire.
    PreExpiryWarning {
        /// The message ID that is expiring.
        message_id: String,
        /// Unix timestamp when the message expires.
        expires_at: u64,
    },

    /// Label sync completed across devices.
    LabelSyncCompleted {
        /// The label ID that was synced.
        label_id: String,
    },

    /// A downgrade (older version) was detected for a contact's delta.
    DowngradeDetected {
        /// The contact ID.
        contact_id: String,
        /// The expected (last applied) version.
        expected_version: u32,
        /// The version that was received.
        received_version: u32,
    },

    /// Sync progress update — emitted for each update processed in a sync cycle.
    SyncProgress {
        /// Total number of updates to process in this cycle.
        total: usize,
        /// Number of updates processed so far (1-indexed).
        processed: usize,
        /// The contact ID of the update just processed.
        contact_id: String,
    },

    /// Error event for async operations.
    Error {
        /// Error description.
        message: String,
    },

    /// Tor bootstrap progress update.
    TorBootstrapProgress {
        /// Bootstrap percentage (0-100).
        percentage: u8,
    },

    /// Tor connection status changed.
    TorStatusChanged {
        /// The new Tor status.
        status: crate::tor_config::TorStatus,
    },

    /// Tor circuit was rotated.
    TorCircuitRotated {
        /// Age of the previous circuit in seconds.
        circuit_age_secs: u64,
    },

    /// Relay health changed (for multi-relay support).
    RelayHealthChanged {
        /// The relay URL whose health changed.
        relay_url: String,
        /// Whether the relay is healthy.
        healthy: bool,
    },

    /// Relay failover occurred.
    RelayFailover {
        /// The relay URL that failed.
        from: String,
        /// The relay URL that was selected as replacement.
        to: String,
    },

    /// A contact was blocked.
    ContactBlocked {
        /// The contact ID.
        contact_id: String,
    },

    /// A contact was unblocked.
    ContactUnblocked {
        /// The contact ID.
        contact_id: String,
    },

    /// Visibility rules changed for a contact, triggering re-propagation.
    VisibilityChanged {
        /// The contact ID whose visibility rules changed.
        contact_id: String,
        /// The field whose visibility changed.
        field: String,
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
