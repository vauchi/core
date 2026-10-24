// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use vauchi_core::platform::Event;
use vauchi_core::platform::executor::{DeliveryError, EventInbox, InboxLimits, Operation};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HardwareFailure {
    Unavailable,
    PermissionDenied,
    Failed,
    InvalidObservation,
}

impl HardwareFailure {
    fn event(self) -> Event {
        match self {
            Self::Unavailable => Event::HardwareUnavailable {
                transport: "BLE".into(),
            },
            Self::PermissionDenied => Event::PermissionDenied {
                transport: "BLE".into(),
            },
            Self::Failed | Self::InvalidObservation => Event::HardwareError {
                transport: "BLE".into(),
                error: match self {
                    Self::InvalidObservation => "Bluetooth observation exceeds executor limits",
                    _ => "Bluetooth discovery failed",
                }
                .into(),
            },
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExecutorError {
    #[error("event budget must be at least 256 bytes to report hardware failures")]
    EventBudgetTooSmall,
    #[error("native discovery worker failed")]
    WorkerFailed,
}

/// OS discovery port. Implementations bound capture buffers before constructing
/// events, and release native discovery when the returned future is dropped.
pub trait Discovery: Send + Sync + 'static {
    fn scan(
        &self,
        service_uuid: String,
        events: EventSender,
    ) -> Pin<Box<dyn Future<Output = Result<(), HardwareFailure>> + Send + '_>>;
}

/// One producer retains at most its current observation while the host drains
/// the inbox. Native discovery must serialize calls to `send`.
pub struct EventSender {
    operation: Operation,
    capacity: Arc<Notify>,
}

impl EventSender {
    pub async fn send(&self, mut event: Event) -> Result<(), DeliveryError> {
        loop {
            let ready = self.capacity.notified();
            tokio::pin!(ready);
            // Register before publishing so a concurrent pop cannot be lost.
            ready.as_mut().enable();
            match self.operation.publish(event) {
                Ok(()) => return Ok(()),
                Err(rejected) if rejected.reason == DeliveryError::Full => {
                    event = rejected.event;
                    ready.await;
                }
                Err(rejected) => return Err(rejected.reason),
            }
        }
    }
}

struct ActiveScan {
    operation: Operation,
    task: JoinHandle<()>,
}

/// Host-owned discovery executor. The host keeps its runtime alive through
/// `shutdown`, and serializes event dispatch with `start`, `stop` and disposal.
pub struct BleScanner {
    backend: Arc<dyn Discovery>,
    runtime: Handle,
    inbox: EventInbox,
    capacity: Arc<Notify>,
    active: Option<ActiveScan>,
}

impl BleScanner {
    pub fn with_backend(
        backend: Arc<dyn Discovery>,
        runtime: Handle,
        limits: InboxLimits,
    ) -> Result<Self, ExecutorError> {
        if limits.event_bytes.get() < 256 {
            return Err(ExecutorError::EventBudgetTooSmall);
        }
        Ok(Self {
            backend,
            runtime,
            inbox: EventInbox::new(limits),
            capacity: Arc::new(Notify::new()),
            active: None,
        })
    }

    /// Replace the native discovery operation. Core chooses its lifetime;
    /// this adapter imposes no scan duration or fallback behavior.
    pub fn start(&mut self, service_uuid: String) -> Result<(), DeliveryError> {
        if service_uuid.len() > 128 {
            return Err(DeliveryError::PayloadTooLarge);
        }
        self.stop();
        let operation = self.inbox.start()?;
        let events = EventSender {
            operation: operation.clone(),
            capacity: self.capacity.clone(),
        };
        let backend = self.backend.clone();
        let completion = operation.clone();
        let task = self.runtime.spawn(async move {
            let failure_events = EventSender {
                operation: completion.clone(),
                capacity: events.capacity.clone(),
            };
            if let Err(failure) = backend.scan(service_uuid, events).await {
                // Static failure events fit the minimum budget. Waiting for
                // capacity preserves failure delivery even after a full queue.
                let _ = failure_events.send(failure.event()).await;
            }
            let _ = completion.finish();
        });
        self.active = Some(ActiveScan { operation, task });
        Ok(())
    }

    pub fn pop(&mut self) -> Option<Event> {
        let event = self.inbox.pop();
        if event.is_some() {
            self.capacity.notify_one();
        }
        event
    }

    pub fn stop(&mut self) {
        if let Some(active) = &self.active {
            let _ = active.operation.cancel();
            active.task.abort();
        }
        self.capacity.notify_waiters();
    }

    /// Close delivery, cancel pending OS work and wait for its future to be
    /// dropped. Native device acceptance must additionally verify OS cleanup.
    pub async fn shutdown(&mut self) -> Result<(), ExecutorError> {
        self.inbox.close();
        self.stop();
        if let Some(active) = self.active.take() {
            match active.task.await {
                Ok(()) => (),
                Err(error) if error.is_cancelled() => (),
                Err(_) => return Err(ExecutorError::WorkerFailed),
            }
        }
        Ok(())
    }
}

impl Drop for BleScanner {
    fn drop(&mut self) {
        self.inbox.close();
        self.stop();
    }
}
