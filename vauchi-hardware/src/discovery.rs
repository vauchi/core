// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::ports::{Discovery, ExecutorError};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use vauchi_core::platform::Event;
use vauchi_core::platform::executor::{DeliveryError, EventInbox, InboxLimits, Operation};

/// Host-owned discovery executor. The host keeps its runtime alive through
/// `shutdown`, and serializes event dispatch with `start`, `stop` and disposal.
pub struct BleScanner {
    inbox: EventInbox,
    capacity: Arc<Notify>,
    available: Arc<Notify>,
    requests: watch::Sender<Option<(String, Operation)>>,
    worker: Option<JoinHandle<()>>,
    active: Option<Operation>,
    closed: bool,
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
        let capacity = Arc::new(Notify::new());
        let available = Arc::new(Notify::new());
        let (requests, receiver) = watch::channel(None);
        let worker = runtime.spawn(crate::worker::run(
            backend,
            receiver,
            capacity.clone(),
            available.clone(),
        ));
        Ok(Self {
            inbox: EventInbox::new(limits),
            capacity,
            available,
            requests,
            worker: Some(worker),
            active: None,
            closed: false,
        })
    }

    /// Replace the native discovery operation. Core chooses its lifetime;
    /// this adapter imposes no scan duration or fallback behavior.
    pub fn start(&mut self, service_uuid: String) -> Result<(), DeliveryError> {
        if self.closed || self.worker.as_ref().is_none_or(JoinHandle::is_finished) {
            return Err(DeliveryError::Closed);
        }
        if service_uuid.len() > 128 {
            return Err(DeliveryError::PayloadTooLarge);
        }
        self.stop();
        let operation = self.inbox.start()?;
        self.requests
            .send_replace(Some((service_uuid, operation.clone())));
        self.active = Some(operation);
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
        if let Some(operation) = self.active.take() {
            operation.cancel();
        }
        self.requests.send_replace(None);
        self.capacity.notify_waiters();
    }

    pub async fn next_event(&mut self) -> Option<Event> {
        let available = self.available.clone();
        loop {
            let ready = available.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            if self.closed {
                return None;
            }
            if let Some(event) = self.pop() {
                return Some(event);
            }
            ready.await;
        }
    }

    /// Close delivery, cancel pending OS work and wait for its future to be
    /// dropped. Native device acceptance must additionally verify OS cleanup.
    pub async fn shutdown(&mut self) -> Result<(), ExecutorError> {
        self.closed = true;
        self.inbox.close();
        self.stop();
        let result = if let Some(worker) = self.worker.as_mut() {
            worker.abort();
            match worker.await {
                Ok(()) => Ok(()),
                Err(error) if error.is_cancelled() => Ok(()),
                Err(_) => Err(ExecutorError::WorkerFailed),
            }
        } else {
            Ok(())
        };
        self.worker.take();
        result
    }
}

impl Drop for BleScanner {
    fn drop(&mut self) {
        self.inbox.close();
        self.stop();
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}
