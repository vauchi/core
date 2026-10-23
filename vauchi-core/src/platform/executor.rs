// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded delivery of platform observations from asynchronous OS callbacks.
//!
//! Handles are local callback lifetimes, never serialized domain identifiers.
//! The host dispatches accepted events into Core and stops native work when
//! cancelling a handle. Cancellation prevents delivery; it does not itself
//! interrupt an OS call. Producers must handle backpressure explicitly.

use super::Event;
use std::collections::{BTreeSet, VecDeque};
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

/// Host resource budgets. Payload size is measured in canonical event JSON
/// bytes without allocating a serialized copy. OS capture buffers must also
/// be bounded by their executor before constructing an event.
#[derive(Clone, Copy, Debug)]
pub struct InboxLimits {
    pub operations: NonZeroUsize,
    pub events: NonZeroUsize,
    pub event_bytes: NonZeroUsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeliveryError {
    #[error("platform event inbox is full")]
    Full,
    #[error("platform operation was cancelled")]
    Cancelled,
    #[error("platform event inbox is closed")]
    Closed,
    #[error("platform event exceeds the payload budget")]
    PayloadTooLarge,
    #[error("platform event cannot be serialized")]
    InvalidEvent,
}

struct State {
    limits: InboxLimits,
    next_id: u64,
    active: BTreeSet<u64>,
    pending: VecDeque<(u64, Event)>,
    closed: bool,
}

impl State {
    fn close(&mut self) {
        self.closed = true;
        self.active.clear();
        self.pending.clear();
    }

    fn check(&self, id: u64) -> Result<(), DeliveryError> {
        if self.closed {
            Err(DeliveryError::Closed)
        } else if !self.active.contains(&id) {
            Err(DeliveryError::Cancelled)
        } else {
            Ok(())
        }
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|poisoned| {
        let mut state = poisoned.into_inner();
        // A panicking producer must not leave observations eligible for delivery.
        state.close();
        state
    })
}

/// A single host-owned inbox. Dropping it releases pending payloads even if
/// native callbacks still retain operation handles.
pub struct EventInbox {
    state: Arc<Mutex<State>>,
}

impl EventInbox {
    pub fn new(limits: InboxLimits) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                limits,
                next_id: 0,
                active: BTreeSet::new(),
                pending: VecDeque::new(),
                closed: false,
            })),
        }
    }

    /// Reserve a callback lifetime before starting native work.
    pub fn start(&mut self) -> Result<Operation, DeliveryError> {
        let mut state = lock(&self.state);
        if state.closed {
            return Err(DeliveryError::Closed);
        }
        if state.active.len() == state.limits.operations.get() {
            return Err(DeliveryError::Full);
        }
        let id = state.next_id;
        state.next_id = id.checked_add(1).ok_or(DeliveryError::Full)?;
        state.active.insert(id);
        Ok(Operation {
            state: Arc::downgrade(&self.state),
            id,
        })
    }

    /// Take the oldest accepted observation. The host must serialize dispatch
    /// and cancellation; an event already popped belongs to that host.
    pub fn pop(&mut self) -> Option<Event> {
        lock(&self.state)
            .pending
            .pop_front()
            .map(|(_, event)| event)
    }

    /// Discard pending observations and permanently reject all producers.
    pub fn close(&mut self) {
        lock(&self.state).close();
    }
}

/// Cloneable callback handle. The host explicitly cancels it when the native
/// operation ends; dropping one callback clone does not end other callbacks.
#[derive(Clone)]
pub struct Operation {
    state: Weak<Mutex<State>>,
    id: u64,
}

impl Operation {
    /// Enqueue an unmodified observation. Full is backpressure, not success:
    /// retain/retry within a bounded producer buffer or terminate native work.
    /// No domain retry or fallback decision belongs in this method.
    pub fn publish(&self, event: Event) -> Result<(), DeliveryError> {
        let shared = self.state.upgrade().ok_or(DeliveryError::Closed)?;
        let limit = {
            let state = lock(&shared);
            state.check(self.id)?;
            state.limits.event_bytes.get()
        };
        validate_size(&event, limit)?;
        let mut state = lock(&shared);
        state.check(self.id)?;
        if state.pending.len() == state.limits.events.get() {
            return Err(DeliveryError::Full);
        }
        state.pending.push_back((self.id, event));
        Ok(())
    }

    /// Invalidate this lifetime and remove its pending observations, leaving
    /// other operations untouched. Repeated cancellation is harmless.
    pub fn cancel(&self) {
        if let Some(shared) = self.state.upgrade() {
            let mut state = lock(&shared);
            state.active.remove(&self.id);
            state.pending.retain(|(id, _)| *id != self.id);
        }
    }
}

struct Budget {
    remaining: usize,
    exceeded: bool,
}

impl Write for Budget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.remaining.checked_sub(bytes.len()) {
            Some(remaining) => {
                self.remaining = remaining;
                Ok(bytes.len())
            }
            None => {
                self.exceeded = true;
                Err(io::Error::other("event payload budget exceeded"))
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate_size(event: &Event, limit: usize) -> Result<(), DeliveryError> {
    let mut budget = Budget {
        remaining: limit,
        exceeded: false,
    };
    serde_json::to_writer(&mut budget, event).map_err(|_| {
        if budget.exceeded {
            DeliveryError::PayloadTooLarge
        } else {
            DeliveryError::InvalidEvent
        }
    })
}
