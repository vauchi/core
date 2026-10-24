// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::num::NonZeroUsize;
use vauchi_core::platform::executor::{DeliveryError, EventInbox, InboxLimits, RejectedEvent};
use vauchi_core::{BleLinkDirection, Event};

fn inbox(operations: usize, events: usize, bytes: usize) -> EventInbox {
    EventInbox::new(InboxLimits {
        operations: NonZeroUsize::new(operations).unwrap(),
        events: NonZeroUsize::new(events).unwrap(),
        event_bytes: NonZeroUsize::new(bytes).unwrap(),
    })
}

fn found(id: &str) -> Event {
    Event::BleDeviceDiscovered {
        id: id.into(),
        rssi: -42,
        adv_data: vec![1, 2, 3],
    }
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_callback_observations_are_delivered_in_acceptance_order() {
    let mut inbox = inbox(2, 3, 1024);
    let first = inbox.start().unwrap();
    let second = inbox.start().unwrap();
    assert_eq!(first.publish(found("one")), Ok(()));
    assert_eq!(second.publish(found("two")), Ok(()));
    assert_eq!(first.publish(found("three")), Ok(()));
    assert_eq!(inbox.pop(), Some(found("one")));
    assert_eq!(inbox.pop(), Some(found("two")));
    assert_eq!(inbox.pop(), Some(found("three")));
    assert_eq!(inbox.pop(), None);
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_cancel_discards_buffered_events_and_rejects_late_callbacks() {
    let mut inbox = inbox(2, 4, 1024);
    let cancelled = inbox.start().unwrap();
    let survivor = inbox.start().unwrap();
    assert_eq!(cancelled.publish(found("stale")), Ok(()));
    assert_eq!(survivor.publish(found("current")), Ok(()));
    cancelled.cancel();
    let replacement = inbox.start().unwrap();
    assert_eq!(
        cancelled.publish(found("late")),
        Err(RejectedEvent {
            reason: DeliveryError::Inactive,
            event: found("late")
        })
    );
    assert_eq!(replacement.publish(found("replacement")), Ok(()));
    assert_eq!(inbox.pop(), Some(found("current")));
    assert_eq!(inbox.pop(), Some(found("replacement")));
    assert_eq!(inbox.pop(), None);
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_cancel_before_first_callback_is_idempotent() {
    let mut inbox = inbox(1, 1, 1024);
    let operation = inbox.start().unwrap();
    operation.cancel();
    operation.cancel();
    assert_eq!(
        operation.publish(found("late")),
        Err(RejectedEvent {
            reason: DeliveryError::Inactive,
            event: found("late")
        })
    );
    assert_eq!(inbox.pop(), None);
    let replacement = inbox.start().unwrap();
    assert_eq!(replacement.publish(found("new")), Ok(()));
    assert_eq!(inbox.pop(), Some(found("new")));
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_full_inbox_reports_backpressure_without_replacing_accepted_events() {
    let mut inbox = inbox(1, 1, 1024);
    let operation = inbox.start().unwrap();
    assert_eq!(inbox.start().err(), Some(DeliveryError::Full));
    assert_eq!(operation.publish(found("first")), Ok(()));
    assert_eq!(
        operation.publish(found("second")),
        Err(RejectedEvent {
            reason: DeliveryError::Full,
            event: found("second")
        })
    );
    assert_eq!(inbox.pop(), Some(found("first")));
    assert_eq!(operation.publish(found("second")), Ok(()));
    assert_eq!(inbox.pop(), Some(found("second")));
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_payload_limit_is_inclusive_and_rejection_does_not_consume_capacity() {
    let event = found("bounded");
    let size = serde_json::to_vec(&event).unwrap().len();
    let mut exact = inbox(1, 1, size);
    assert_eq!(exact.start().unwrap().publish(event.clone()), Ok(()));
    assert_eq!(exact.pop(), Some(event.clone()));
    let mut smaller = inbox(1, 1, size - 1);
    let operation = smaller.start().unwrap();
    assert_eq!(
        operation.publish(event.clone()),
        Err(RejectedEvent {
            reason: DeliveryError::PayloadTooLarge,
            event
        })
    );
    let short = Event::HardwareUnavailable {
        transport: "BLE".into(),
    };
    assert_eq!(operation.publish(short.clone()), Ok(()));
    assert_eq!(smaller.pop(), Some(short));
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_shutdown_discards_pending_events_and_closes_existing_and_new_producers() {
    let mut inbox = inbox(1, 2, 1024);
    let operation = inbox.start().unwrap();
    assert_eq!(operation.publish(found("pending")), Ok(()));
    inbox.close();
    inbox.close();
    assert_eq!(inbox.pop(), None);
    assert_eq!(
        operation.publish(found("late")),
        Err(RejectedEvent {
            reason: DeliveryError::Closed,
            event: found("late")
        })
    );
    assert_eq!(inbox.start().err(), Some(DeliveryError::Closed));
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_dropping_inbox_closes_callbacks_without_retaining_queued_payloads() {
    let operation = {
        let mut inbox = inbox(1, 1, 1024);
        inbox.start().unwrap()
    };
    assert_eq!(
        operation.publish(found("late")),
        Err(RejectedEvent {
            reason: DeliveryError::Closed,
            event: found("late")
        })
    );
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_permission_failure_and_disconnect_observations_are_not_reinterpreted() {
    let mut inbox = inbox(1, 1, 1024);
    let operation = inbox.start().unwrap();
    for event in [
        Event::PermissionDenied {
            transport: "BLE".into(),
        },
        Event::HardwareUnavailable {
            transport: "BLE".into(),
        },
        Event::HardwareError {
            transport: "BLE".into(),
            error: "OS failure".into(),
        },
        Event::BleDisconnected {
            device_id: "peer".into(),
            direction: BleLinkDirection::Outbound,
            reason: "link closed".into(),
        },
    ] {
        assert_eq!(operation.publish(event.clone()), Ok(()));
        assert_eq!(inbox.pop(), Some(event));
    }
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_cancelled_callback_on_another_thread_cannot_reach_a_replacement_operation() {
    let mut inbox = inbox(1, 1, 1024);
    let operation = inbox.start().unwrap();
    let callback = operation.clone();
    let (release, wait) = std::sync::mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        wait.recv().unwrap();
        callback.publish(found("late"))
    });
    operation.cancel();
    let replacement = inbox.start().unwrap();
    release.send(()).unwrap();
    assert_eq!(
        worker.join().unwrap(),
        Err(RejectedEvent {
            reason: DeliveryError::Inactive,
            event: found("late")
        })
    );
    assert_eq!(replacement.publish(found("replacement")), Ok(()));
    assert_eq!(inbox.pop(), Some(found("replacement")));
    assert_eq!(inbox.pop(), None);
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_finish_preserves_final_observation_and_releases_operation_capacity() {
    let mut inbox = inbox(1, 2, 1024);
    let operation = inbox.start().unwrap();
    assert_eq!(operation.publish(found("final")), Ok(()));
    assert_eq!(operation.finish(), Ok(()));
    let next = inbox.start().unwrap();
    assert_eq!(next.publish(found("next")), Ok(()));
    assert_eq!(
        operation.publish(found("late")).unwrap_err().event,
        found("late")
    );
    assert_eq!(inbox.pop(), Some(found("final")));
    assert_eq!(inbox.pop(), Some(found("next")));
    assert_eq!(inbox.pop(), None);
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn test_backpressure_returns_the_rejected_event_for_retry() {
    let mut inbox = inbox(1, 1, 1024);
    let operation = inbox.start().unwrap();
    assert_eq!(operation.publish(found("first")), Ok(()));
    let rejected = operation.publish(found("retry")).unwrap_err();
    assert_eq!(rejected.reason, DeliveryError::Full);
    assert_eq!(rejected.event, found("retry"));
    assert_eq!(inbox.pop(), Some(found("first")));
    assert_eq!(operation.publish(rejected.event), Ok(()));
    assert_eq!(inbox.pop(), Some(found("retry")));
}
