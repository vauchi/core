// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use proptest::prelude::*;
use vauchi_app::orchestrator::recovery_transfer::{RecoveryTransferSender, TransferState};
use vauchi_core::exchange::{CHAR_DATA_NOTIFY, CHAR_DATA_WRITE};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::platform::executor::DeliveryError;
use vauchi_core::sync::guardian_piece::split_into_pieces;
use vauchi_core::sync::guardian_transfer::{HandBackReceiver, TransferError};
use vauchi_core::{Command, Event};

// @internal — DC-01: the selected link is bounded before allocating its inbox.
#[test]
fn recovery_sender_rejects_invalid_links_mtu_and_clock_overflow() {
    let piece = split_into_pieces([1; 16], b"ciphertext").unwrap().remove(0);
    for device in [String::new(), "x".repeat(129)] {
        assert!(matches!(
            RecoveryTransferSender::new(&piece, 23, device, BleLinkDirection::Outbound),
            Err(TransferError::State)
        ));
    }
    for mtu in [0, 22, 518, u16::MAX] {
        assert!(matches!(
            RecoveryTransferSender::new(&piece, mtu, "peer".into(), BleLinkDirection::Outbound),
            Err(TransferError::Frame)
        ));
    }
    let mut sender =
        RecoveryTransferSender::new(&piece, 23, "peer".into(), BleLinkDirection::Outbound).unwrap();
    assert_eq!(
        sender.poll(u64::MAX),
        vec![Command::BleDisconnect {
            device_id: "peer".into(),
            direction: BleLinkDirection::Outbound,
        }]
    );
    assert_eq!(sender.state(), TransferState::Failed);
}

proptest! {
    // @internal — CC-04: both GATT roles honor every valid ATT MTU.
    #[test]
    fn recovery_sender_roundtrips_pieces_at_every_mtu(
        mtu in 23u16..=517,
        body in prop::collection::vec(any::<u8>(), 1..512),
        inbound in any::<bool>(),
    ) {
        let direction = if inbound { BleLinkDirection::Inbound } else { BleLinkDirection::Outbound };
        let write_uuid = if inbound { CHAR_DATA_NOTIFY } else { CHAR_DATA_WRITE };
        let ack_uuid = if inbound { CHAR_DATA_WRITE } else { CHAR_DATA_NOTIFY };
        let piece = split_into_pieces([1; 16], &body).unwrap().remove(0);
        let mut sender = RecoveryTransferSender::new(&piece, mtu, "peer".into(), direction).unwrap();
        let callback = sender.callback();
        let mut receiver = HandBackReceiver::new(piece.hash());
        let mut received = None;
        let mut writes = 0;
        let max_writes = piece.encode().len().div_ceil(usize::from(mtu).min(515) - 13);
        while sender.state() == TransferState::Sending {
            let commands = sender.poll(100);
            if sender.state() == TransferState::Acknowledged {
                prop_assert_eq!(commands, vec![Command::BleDisconnect {
                    device_id: "peer".into(), direction,
                }]);
                break;
            }
            let [Command::BleWriteCharacteristic { device_id, direction: link, uuid, data }, wakeup] =
                commands.as_slice() else { prop_assert!(false, "expected a paced write"); return Ok(()); };
            prop_assert_eq!(device_id, "peer");
            prop_assert_eq!(*link, direction);
            prop_assert_eq!(uuid, write_uuid);
            prop_assert_eq!(wakeup, &Command::schedule_wakeup(5, 5, 0, None));
            prop_assert!(data.len() <= usize::from(mtu).saturating_sub(3).min(512));
            writes += 1;
            prop_assert!(writes <= max_writes);
            let receipt = receiver.receive(data).unwrap();
            if let Some(piece) = receipt.piece() { received = Some(piece.clone()); }
            callback.publish(Event::BleCharacteristicRead {
                device_id: "peer".into(), direction, uuid: ack_uuid.into(),
                data: receipt.acknowledgement().to_vec(),
            }).unwrap();
        }
        prop_assert_eq!(sender.state(), TransferState::Acknowledged);
        prop_assert_eq!(received, Some(piece));
        prop_assert_eq!(writes, max_writes);
    }

    // @internal — CC-13: noise, ticks and cancellation never acknowledge a piece.
    #[test]
    fn recovery_sender_random_event_sequences_cannot_advance_without_an_ack(
        operations in prop::collection::vec((0u8..4, 0u64..8, prop::collection::vec(any::<u8>(), 0..24)), 0..80),
    ) {
        let piece = split_into_pieces([1; 16], b"ciphertext").unwrap().remove(0);
        let mut sender = RecoveryTransferSender::new(&piece, 23, "peer".into(),
            BleLinkDirection::Outbound).unwrap();
        let callback = sender.callback();
        let first = sender.poll(0);
        let mut now = 0;
        let mut terminal = None;
        for (operation, elapsed, data) in operations {
            now += elapsed;
            let event = match operation {
                0 => Event::BleCharacteristicNotified {
                    device_id: "other peer".into(), direction: BleLinkDirection::Outbound,
                    uuid: CHAR_DATA_NOTIFY.into(), data,
                },
                1 => Event::WakeupFired,
                _ => Event::BleDisconnected {
                    device_id: "other peer".into(), direction: BleLinkDirection::Outbound,
                    reason: "closed".into(),
                },
            };
            match callback.publish(event) {
                Ok(()) => prop_assert_eq!(sender.state(), TransferState::Sending),
                Err(error) => prop_assert_eq!(error.reason, DeliveryError::Inactive),
            }
            let commands = if operation == 3 { sender.cancel() } else { sender.poll(now) };
            if matches!(commands.first(), Some(Command::BleWriteCharacteristic { .. })) {
                prop_assert_eq!(&commands, &first);
            }
            prop_assert_ne!(sender.state(), TransferState::Acknowledged);
            if let Some(state) = terminal {
                prop_assert_eq!(sender.state(), state);
                prop_assert!(commands.is_empty());
            } else if sender.state() != TransferState::Sending {
                terminal = Some(sender.state());
            }
        }
    }
}
