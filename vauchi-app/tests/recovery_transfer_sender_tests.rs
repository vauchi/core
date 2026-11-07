// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::orchestrator::recovery_transfer::{RecoveryTransferSender, TransferState};
use vauchi_core::exchange::{CHAR_DATA_NOTIFY, CHAR_DATA_WRITE};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::platform::executor::DeliveryError;
use vauchi_core::sync::guardian_piece::split_into_pieces;
use vauchi_core::sync::guardian_transfer::HandBackReceiver;
use vauchi_core::{Command, Event};

const PEER: &str = "guardian-link";

fn sender() -> RecoveryTransferSender {
    let pieces = split_into_pieces([7; 16], b"encrypted recovery backup").unwrap();
    RecoveryTransferSender::new(&pieces[0], 23, PEER.into(), BleLinkDirection::Outbound).unwrap()
}

fn packet(commands: &[Command]) -> Vec<u8> {
    match commands {
        [
            Command::BleWriteCharacteristic {
                device_id,
                direction,
                uuid,
                data,
            },
            wakeup,
        ] => {
            assert_eq!(device_id, PEER);
            assert_eq!(*direction, BleLinkDirection::Outbound);
            assert_eq!(uuid, CHAR_DATA_WRITE);
            assert_eq!(wakeup, &Command::schedule_wakeup(5, 5, 0, None));
            assert!(data.len() <= 20);
            data.clone()
        }
        other => panic!("expected one bounded write and its deadline, got {other:?}"),
    }
}

fn ack(data: Vec<u8>) -> Event {
    Event::BleCharacteristicNotified {
        device_id: PEER.into(),
        direction: BleLinkDirection::Outbound,
        uuid: CHAR_DATA_NOTIFY.into(),
        data,
    }
}

fn disconnect() -> Vec<Command> {
    vec![Command::BleDisconnect {
        device_id: PEER.into(),
        direction: BleLinkDirection::Outbound,
    }]
}

// @internal — P33 sender command/event slice; the ceremony scenario stays planned.
#[test]
fn recovery_sender_paces_small_packets_until_the_piece_is_acknowledged() {
    let pieces = split_into_pieces([7; 16], b"encrypted recovery backup").unwrap();
    let mut receiver = HandBackReceiver::new(pieces[0].hash());
    let mut sender = sender();
    let callback = sender.callback();
    let mut commands = sender.poll(100);
    let mut received = None;
    loop {
        let data = packet(&commands);
        assert_eq!(sender.poll(100), Vec::<Command>::new());
        let receipt = receiver.receive(&data).unwrap();
        if let Some(piece) = receipt.piece() {
            received = Some(piece.clone());
        }
        callback
            .publish(ack(receipt.acknowledgement().to_vec()))
            .unwrap();
        commands = sender.poll(100);
        if sender.state() == TransferState::Acknowledged {
            break;
        }
    }
    assert_eq!(received, Some(pieces[0].clone()));
    assert_eq!(commands, disconnect());
    assert_eq!(sender.poll(500), Vec::<Command>::new());
    assert_eq!(
        callback.publish(ack(vec![])).unwrap_err().reason,
        DeliveryError::Inactive
    );
}

// @internal — DC-02: peer, direction and characteristic are all part of the link.
#[test]
fn recovery_sender_ignores_acknowledgements_from_other_links() {
    let mut sender = sender();
    let callback = sender.callback();
    let first = packet(&sender.poll(100));
    let valid = b"RBA1\0\0\0\x0a".to_vec();
    let events = [
        Event::BleCharacteristicNotified {
            device_id: "another peer".into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_DATA_NOTIFY.into(),
            data: valid.clone(),
        },
        Event::BleCharacteristicNotified {
            device_id: PEER.into(),
            direction: BleLinkDirection::Inbound,
            uuid: CHAR_DATA_NOTIFY.into(),
            data: valid.clone(),
        },
        Event::BleCharacteristicNotified {
            device_id: PEER.into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_DATA_WRITE.into(),
            data: valid.clone(),
        },
    ];
    for event in events {
        callback.publish(event).unwrap();
        assert_eq!(sender.poll(101), Vec::<Command>::new());
    }
    assert_eq!(packet(&sender.poll(105)), first);
    callback.publish(ack(valid)).unwrap();
    assert_ne!(packet(&sender.poll(106)), first);
}

// @internal — P33 cancellation emits native teardown and invalidates the callback.
#[test]
fn recovery_sender_cancellation_discards_queued_and_late_acknowledgements() {
    let mut old = sender();
    let callback = old.callback();
    packet(&old.poll(100));
    callback.publish(ack(b"RBA1\0\0\0\x0a".to_vec())).unwrap();
    assert_eq!(old.cancel(), disconnect());
    assert_eq!(old.state(), TransferState::Cancelled);
    assert_eq!(old.poll(105), Vec::<Command>::new());
    assert_eq!(old.cancel(), Vec::<Command>::new());

    let mut replacement = sender();
    let first = packet(&replacement.poll(200));
    assert_eq!(
        callback
            .publish(ack(b"RBA1\0\0\0\x0a".to_vec()))
            .unwrap_err()
            .reason,
        DeliveryError::Inactive
    );
    assert_eq!(replacement.poll(201), Vec::<Command>::new());
    assert_eq!(packet(&replacement.poll(205)), first);
}

// @internal — DC-02: retries have a finite Core-owned budget.
#[test]
fn recovery_sender_retries_only_at_its_deadline_and_eventually_disconnects() {
    let mut sender = sender();
    let first = packet(&sender.poll(100));
    for deadline in [105, 110, 115] {
        assert_eq!(sender.poll(deadline - 1), Vec::<Command>::new());
        assert_eq!(packet(&sender.poll(deadline)), first);
    }
    assert_eq!(sender.poll(120), disconnect());
    assert_eq!(sender.state(), TransferState::Failed);
    assert_eq!(sender.poll(200), Vec::<Command>::new());
}

// @internal — DC-01/DC-02: malformed or never-issued ACKs terminate the transfer.
#[test]
fn recovery_sender_refuses_malformed_and_out_of_order_acknowledgements() {
    for data in [
        vec![],
        b"RBA1\0\0\0".to_vec(),
        b"RBA1\0\0\0\x0a\0".to_vec(),
        b"RBA1\0\x01\0\x0a".to_vec(),
        b"RBA1\0\0\0\x14".to_vec(),
        b"RBA1\0\0\0\x01".to_vec(),
    ] {
        let mut sender = sender();
        let callback = sender.callback();
        packet(&sender.poll(100));
        callback.publish(ack(data)).unwrap();
        assert_eq!(sender.poll(101), disconnect());
        assert_eq!(sender.state(), TransferState::Failed);
        assert_eq!(sender.poll(200), Vec::<Command>::new());
    }
}

// @internal — DC-02: scoped native failure cannot look like successful delivery.
#[test]
fn recovery_sender_stops_on_scoped_hardware_failure_or_link_loss() {
    for event in [
        Event::HardwareError {
            transport: "BLE".into(),
            error: "private native error".into(),
        },
        Event::HardwareUnavailable {
            transport: "BLE".into(),
        },
        Event::PermissionDenied {
            transport: "BLE".into(),
        },
        Event::BleDisconnected {
            device_id: PEER.into(),
            direction: BleLinkDirection::Outbound,
            reason: "private native reason".into(),
        },
    ] {
        let mut sender = sender();
        let callback = sender.callback();
        packet(&sender.poll(100));
        callback.publish(event).unwrap();
        assert_eq!(sender.poll(101), disconnect());
        assert_eq!(sender.state(), TransferState::Failed);
        assert_eq!(
            callback.publish(ack(vec![])).unwrap_err().reason,
            DeliveryError::Inactive
        );
    }
}

// @internal — Android reports `ble` where iOS reports `BLE`; both must end the transfer.
#[test]
fn recovery_sender_stops_on_a_failure_named_in_any_casing() {
    for event in [
        Event::HardwareError {
            transport: "ble".into(),
            error: "private native error".into(),
        },
        Event::HardwareUnavailable {
            transport: "ble".into(),
        },
        Event::PermissionDenied {
            transport: "Ble".into(),
        },
    ] {
        let mut sender = sender();
        let callback = sender.callback();
        packet(&sender.poll(100));
        callback.publish(event).unwrap();
        assert_eq!(sender.poll(101), disconnect());
        assert_eq!(sender.state(), TransferState::Failed);
    }
}

// @internal — another transport's failure says nothing about this link.
#[test]
fn recovery_sender_ignores_another_transports_failure() {
    let mut sender = sender();
    let callback = sender.callback();
    packet(&sender.poll(100));
    let _ = callback.publish(Event::HardwareUnavailable {
        transport: "camera".into(),
    });
    assert_eq!(sender.poll(101), Vec::<Command>::new());
    assert_eq!(sender.state(), TransferState::Sending);
}

// @internal — repeated old ACKs cannot renew the deadline for a pending packet.
#[test]
fn recovery_sender_duplicate_acknowledgements_do_not_postpone_retry() {
    let mut sender = sender();
    let callback = sender.callback();
    packet(&sender.poll(100));
    callback.publish(ack(b"RBA1\0\0\0\x0a".to_vec())).unwrap();
    let second = packet(&sender.poll(101));
    for now in 102..106 {
        callback.publish(ack(b"RBA1\0\0\0\x0a".to_vec())).unwrap();
        assert_eq!(sender.poll(now), Vec::<Command>::new());
    }
    assert_eq!(packet(&sender.poll(106)), second);
}

// @internal — callback budgets bound pending data and preserve rejected ownership.
#[test]
fn recovery_sender_callback_applies_backpressure_and_payload_bounds() {
    let mut sender = sender();
    let callback = sender.callback();
    let oversized = ack(vec![0; 4096]);
    let rejected = callback.publish(oversized.clone()).unwrap_err();
    assert_eq!(rejected.reason, DeliveryError::PayloadTooLarge);
    assert_eq!(rejected.event, oversized);
    for _ in 0..4 {
        callback.publish(Event::WakeupFired).unwrap();
    }
    let rejected = callback.publish(Event::WakeupFired).unwrap_err();
    assert_eq!(rejected.reason, DeliveryError::Full);
    assert_eq!(rejected.event, Event::WakeupFired);
    packet(&sender.poll(100));
    callback.publish(rejected.event).unwrap();
    assert_eq!(sender.poll(101), Vec::<Command>::new());
    assert_eq!(sender.state(), TransferState::Sending);
}

// @internal — DC-01: a shell reports an inbound link with no id; a dialled link must name its peer.
#[test]
fn only_an_inbound_link_may_have_no_id() {
    let pieces = split_into_pieces([7; 16], b"encrypted recovery backup").unwrap();
    let accepts = |peer: &str, direction| {
        RecoveryTransferSender::new(&pieces[0], 23, peer.into(), direction).is_ok()
    };

    assert!(accepts("", BleLinkDirection::Inbound));
    assert!(!accepts("", BleLinkDirection::Outbound));
    assert!(accepts("peer", BleLinkDirection::Outbound));
    assert!(!accepts(&"x".repeat(129), BleLinkDirection::Inbound));
}
