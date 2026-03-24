// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Event System
//!
//! Callbacks for Vauchi events.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::network::ConnectionState;
use crate::sync::SyncState;

/// Unique identifier for a registered event handler.
pub type HandlerId = u64;

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

    /// A contact was hidden from the main list.
    ContactHidden {
        /// The contact ID.
        contact_id: String,
    },

    /// A contact was unhidden (returned to the main list).
    ContactUnhidden {
        /// The contact ID.
        contact_id: String,
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

    /// An emergency alert was received from a contact.
    EmergencyAlertReceived {
        /// The contact ID who sent the alert.
        contact_id: String,
        /// The alert message.
        message: String,
        /// Unix timestamp when the alert was created.
        timestamp: u64,
        /// Optional location as (latitude, longitude).
        location: Option<(f64, f64)>,
    },

    /// An emergency broadcast was sent.
    EmergencyBroadcastSent {
        /// Number of alerts successfully queued for delivery.
        sent_count: usize,
        /// Total number of trusted contacts in the config.
        total: usize,
    },

    /// A contact's field was validated by someone.
    FieldValidated {
        /// The contact whose field was validated.
        contact_id: String,
        /// The field that was validated.
        field_id: String,
        /// The validator's contact ID.
        validator_id: String,
    },

    /// A validation was revoked.
    FieldValidationRevoked {
        /// The contact whose field validation was revoked.
        contact_id: String,
        /// The field whose validation was revoked.
        field_id: String,
        /// The validator who revoked.
        validator_id: String,
    },

    /// A validated field's value changed, resetting its validations.
    FieldValidationReset {
        /// The contact whose field changed.
        contact_id: String,
        /// The field that changed.
        field_id: String,
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

/// Event dispatcher for managing multiple handlers (#87, #89, #94).
///
/// Uses interior mutability (`Mutex`) so that `add_handler` and `remove_handler`
/// take `&self` instead of `&mut self`. This allows handler registration even
/// when the dispatcher is shared via `Arc` (e.g., with `SyncController`).
pub struct EventDispatcher {
    handlers: Mutex<Vec<(HandlerId, Arc<dyn EventHandler>)>>,
    next_id: AtomicU64,
}

impl Default for EventDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl EventDispatcher {
    /// Creates a new event dispatcher.
    pub fn new() -> Self {
        EventDispatcher {
            handlers: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Adds an event handler and returns its unique ID (#87, #94).
    ///
    /// Takes `&self` (not `&mut self`) thanks to interior mutability.
    /// The returned `HandlerId` can be used with `remove_handler()`.
    pub fn add_handler(&self, handler: Arc<dyn EventHandler>) -> HandlerId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut handlers = self.handlers.lock().expect("EventDispatcher lock poisoned");
        handlers.push((id, handler));
        id
    }

    /// Removes a handler by its ID (#89). Returns true if a handler was removed.
    pub fn remove_handler(&self, id: HandlerId) -> bool {
        let mut handlers = self.handlers.lock().expect("EventDispatcher lock poisoned");
        let len_before = handlers.len();
        handlers.retain(|(hid, _)| *hid != id);
        handlers.len() < len_before
    }

    /// Removes all handlers.
    pub fn clear_handlers(&self) {
        let mut handlers = self.handlers.lock().expect("EventDispatcher lock poisoned");
        handlers.clear();
    }

    /// Returns the number of registered handlers.
    pub fn handler_count(&self) -> usize {
        let handlers = self.handlers.lock().expect("EventDispatcher lock poisoned");
        handlers.len()
    }

    /// Dispatches an event to all handlers.
    ///
    /// NOTE (#71): Dispatch is synchronous — a slow handler blocks the caller.
    /// For non-blocking dispatch, callers should spawn handler execution on a
    /// separate thread or use an async event channel.
    pub fn dispatch(&self, event: VauchiEvent) {
        let handlers = self.handlers.lock().expect("EventDispatcher lock poisoned");
        for (_, handler) in handlers.iter() {
            handler.on_event(event.clone());
        }
    }
}
