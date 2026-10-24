// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::delivery::EventSender;
use crate::ports::{Discovery, HardwareFailure};
use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio::sync::{Notify, watch};
use vauchi_core::platform::executor::Operation;

pub(crate) async fn run(
    backend: Arc<dyn Discovery>,
    mut requests: watch::Receiver<Option<(String, Operation)>>,
    capacity: Arc<Notify>,
    available: Arc<Notify>,
) {
    loop {
        let request = requests.borrow_and_update().clone();
        if let Some((service_uuid, operation)) = request {
            tokio::select! {
                biased;
                changed = requests.changed() => {
                    if changed.is_err() { return; }
                }
                () = scan(&*backend, service_uuid, operation, capacity.clone(), available.clone()) => {
                    if requests.changed().await.is_err() { return; }
                }
            }
        } else if requests.changed().await.is_err() {
            return;
        }
    }
}

async fn scan(
    backend: &dyn Discovery,
    service_uuid: String,
    operation: Operation,
    capacity: Arc<Notify>,
    available: Arc<Notify>,
) {
    let events = EventSender {
        operation: operation.clone(),
        capacity: capacity.clone(),
        available: available.clone(),
    };
    let outcome = AssertUnwindSafe(async { backend.scan(service_uuid, events).await })
        .catch_unwind()
        .await
        .unwrap_or(Err(HardwareFailure::Failed));
    if let Err(failure) = outcome {
        let events = EventSender {
            operation: operation.clone(),
            capacity,
            available,
        };
        // The constructor reserves enough payload space for these static
        // failure events. Capacity is awaited, never silently discarded.
        let _ = events.send(failure.event()).await;
    }
    let _ = operation.finish();
}
