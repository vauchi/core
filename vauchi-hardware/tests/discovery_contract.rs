// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use vauchi_core::platform::Event;
use vauchi_core::platform::executor::{DeliveryError, InboxLimits};
use vauchi_hardware::{BleScanner, Discovery, EventSender, HardwareFailure};

struct Observation {
    event: Event,
    entered: oneshot::Sender<()>,
    delivered: oneshot::Sender<Result<(), DeliveryError>>,
}

struct NativeDiscovery {
    input: Mutex<Option<mpsc::Receiver<Observation>>>,
    started: mpsc::UnboundedSender<String>,
    released: mpsc::UnboundedSender<()>,
    failure: Option<HardwareFailure>,
}

struct Release(mpsc::UnboundedSender<()>);

impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

impl Discovery for NativeDiscovery {
    fn scan(
        &self,
        service_uuid: String,
        events: EventSender,
    ) -> Pin<Box<dyn Future<Output = Result<(), HardwareFailure>> + Send + '_>> {
        Box::pin(async move {
            let _release = Release(self.released.clone());
            self.started.send(service_uuid).unwrap();
            if let Some(failure) = &self.failure {
                return Err(failure.clone());
            }
            let mut input = self.input.lock().unwrap().take().unwrap();
            while let Some(observation) = input.recv().await {
                observation.entered.send(()).unwrap();
                let result = events.send(observation.event).await;
                observation.delivered.send(result).unwrap();
            }
            Ok(())
        })
    }
}

fn event(id: &str) -> Event {
    Event::BleDeviceDiscovered {
        id: id.into(),
        rssi: -42,
        adv_data: vec![1, 2, 3],
    }
}

fn limits() -> InboxLimits {
    InboxLimits {
        operations: NonZeroUsize::new(1).unwrap(),
        events: NonZeroUsize::new(1).unwrap(),
        event_bytes: NonZeroUsize::new(512).unwrap(),
    }
}

struct Fixture {
    scanner: BleScanner,
    input: mpsc::Sender<Observation>,
    started: mpsc::UnboundedReceiver<String>,
    released: mpsc::UnboundedReceiver<()>,
}

fn fixture(failure: Option<HardwareFailure>) -> Fixture {
    let (input, receiver) = mpsc::channel(1);
    let (started_tx, started) = mpsc::unbounded_channel();
    let (released_tx, released) = mpsc::unbounded_channel();
    let backend = NativeDiscovery {
        input: Mutex::new(Some(receiver)),
        started: started_tx,
        released: released_tx,
        failure,
    };
    Fixture {
        scanner: BleScanner::with_backend(
            Arc::new(backend),
            tokio::runtime::Handle::current(),
            limits(),
        )
        .unwrap(),
        input,
        started,
        released,
    }
}

async fn feed(
    input: &mpsc::Sender<Observation>,
    event: Event,
) -> oneshot::Receiver<Result<(), DeliveryError>> {
    let (entered_tx, entered) = oneshot::channel();
    let (delivered_tx, delivered) = oneshot::channel();
    input
        .send(Observation {
            event,
            entered: entered_tx,
            delivered: delivered_tx,
        })
        .await
        .unwrap();
    entered.await.unwrap();
    delivered
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn observations_preserve_bytes_order_and_backpressure() {
    let mut f = fixture(None);
    f.scanner.start("service-from-core".into()).unwrap();
    assert_eq!(f.started.recv().await.as_deref(), Some("service-from-core"));
    assert_eq!(feed(&f.input, event("first")).await.await.unwrap(), Ok(()));
    let mut second = feed(&f.input, event("second")).await;
    assert_eq!(second.try_recv(), Err(oneshot::error::TryRecvError::Empty));
    assert_eq!(f.scanner.pop(), Some(event("first")));
    assert_eq!(second.await.unwrap(), Ok(()));
    assert_eq!(f.scanner.pop(), Some(event("second")));
    assert_eq!(f.scanner.pop(), None);
    f.scanner.shutdown().await.unwrap();
    assert_eq!(f.released.recv().await, Some(()));
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn shutdown_discards_queued_events_and_releases_a_backpressured_driver() {
    let mut f = fixture(None);
    f.scanner.start("service".into()).unwrap();
    assert_eq!(f.started.recv().await.as_deref(), Some("service"));
    assert_eq!(feed(&f.input, event("queued")).await.await.unwrap(), Ok(()));
    let mut blocked = feed(&f.input, event("blocked")).await;
    assert_eq!(blocked.try_recv(), Err(oneshot::error::TryRecvError::Empty));
    f.scanner.shutdown().await.unwrap();
    assert_eq!(f.released.recv().await, Some(()));
    assert_eq!(f.scanner.pop(), None);
    assert_eq!(
        blocked.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
    assert_eq!(f.scanner.start("late".into()), Err(DeliveryError::Closed));
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn permission_denial_is_forwarded_and_normal_completion_releases_the_slot() {
    let mut f = fixture(Some(HardwareFailure::PermissionDenied));
    for _ in 0..2 {
        f.scanner.start("service".into()).unwrap();
        assert_eq!(f.started.recv().await.as_deref(), Some("service"));
        assert_eq!(f.released.recv().await, Some(()));
        assert_eq!(
            f.scanner.pop(),
            Some(Event::PermissionDenied {
                transport: "BLE".into()
            })
        );
    }
    f.scanner.shutdown().await.unwrap();
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn dropping_the_owner_cancels_pending_native_work() {
    let mut f = fixture(None);
    f.scanner.start("service".into()).unwrap();
    assert_eq!(f.started.recv().await.as_deref(), Some("service"));
    drop(f.scanner);
    assert_eq!(f.released.recv().await, Some(()));
    let rejected = f
        .input
        .send(Observation {
            event: event("late"),
            entered: oneshot::channel().0,
            delivered: oneshot::channel().0,
        })
        .await
        .unwrap_err()
        .0;
    assert_eq!(rejected.event, event("late"));
}
