// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;
use tokio::sync::Notify;
use vauchi_core::platform::Event;
use vauchi_core::platform::executor::{DeliveryError, Operation};

/// One producer retains at most its current observation while the host drains
/// the inbox. Native discovery must serialize calls to `send`.
pub struct EventSender {
    pub(crate) operation: Operation,
    pub(crate) capacity: Arc<Notify>,
    pub(crate) available: Arc<Notify>,
}

impl EventSender {
    pub async fn send(&self, mut event: Event) -> Result<(), DeliveryError> {
        loop {
            let ready = self.capacity.notified();
            tokio::pin!(ready);
            // Register before publishing so a concurrent pop cannot be lost.
            ready.as_mut().enable();
            match self.operation.publish(event) {
                Ok(()) => {
                    self.available.notify_one();
                    return Ok(());
                }
                Err(rejected) if rejected.reason == DeliveryError::Full => {
                    event = rejected.event;
                    ready.await;
                }
                Err(rejected) => return Err(rejected.reason),
            }
        }
    }
}
