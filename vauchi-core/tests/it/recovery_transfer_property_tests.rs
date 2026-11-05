// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::sync::guardian_piece::split_backup_for_hand_back;
use vauchi_core::sync::guardian_transfer::{HandBackReceiver, HandBackSender, TransferError};

// @internal — A11 wire vectors freeze magic, field order and big-endian offsets.
#[test]
fn data_headers_and_acknowledgements_match_the_published_wire_vectors() {
    let pieces = split_backup_for_hand_back(&vec![7; 49_101]).unwrap();
    let piece = &pieces[1];
    let encoded = piece.encode();
    assert_eq!(encoded.len(), 53);
    let mut sender = HandBackSender::new(piece, 23).unwrap();
    let mut receiver = HandBackReceiver::new(piece.hash());

    let first = sender.next_packet().unwrap().unwrap();
    assert_eq!(&first[..10], b"RBP1\x00\x01\x00\x00\x00\x35");
    assert_eq!(&first[10..], &encoded[..10]);
    let receipt = receiver.receive(&first).unwrap();
    assert_eq!(receipt.acknowledgement(), b"RBA1\x00\x01\x00\x0a");
    sender.acknowledge(receipt.acknowledgement()).unwrap();

    let second = sender.next_packet().unwrap().unwrap();
    assert_eq!(&second[..10], b"RBP1\x00\x01\x00\x0a\x00\x35");
    assert_eq!(&second[10..], &encoded[10..20]);
}

proptest! {
    // @internal
    #[test]
    fn prop_pieces_cross_varying_mtus_with_duplicate_data_and_acks(
        backup in proptest::collection::vec(any::<u8>(), 1..5000),
        mtu in 23u16..=517,
        duplicate in any::<bool>(),
    ) {
        let pieces = split_backup_for_hand_back(&backup).unwrap();
        let piece = &pieces[0];
        let mut sender = HandBackSender::new(piece, mtu).unwrap();
        let mut receiver = HandBackReceiver::new(piece.hash());
        let mut completed = None;
        let max_packets = piece.encode().len();
        let mut packets = 0;
        while !sender.is_complete() {
            packets += 1;
            prop_assert!(packets <= max_packets);
            let packet = sender.next_packet().unwrap().unwrap();
            prop_assert!(packet.len() <= (usize::from(mtu) - 3).min(512));
            prop_assert_eq!(sender.next_packet().unwrap(), None);
            let receipt = receiver.receive(&packet).unwrap();
            if duplicate {
                prop_assert_eq!(sender.retry_packet().unwrap(), packet.clone());
                let repeated = receiver.receive(&packet).unwrap();
                prop_assert_eq!(&repeated, &receipt);
            }
            completed = receipt.piece().cloned().or(completed);
            sender.acknowledge(receipt.acknowledgement()).unwrap();
            if duplicate {
                sender.acknowledge(receipt.acknowledgement()).unwrap();
            }
        }
        prop_assert_eq!(completed.as_ref(), Some(piece));
    }

    // @internal — CC-13: one outstanding packet and permanent cancellation.
    #[test]
    fn prop_transfer_operations_follow_the_stop_and_wait_model(
        actions in proptest::collection::vec(0u8..6, 0..128),
        mtu in 23u16..=100,
    ) {
        let pieces = split_backup_for_hand_back(&[5; 30]).unwrap();
        let mut sender = HandBackSender::new(&pieces[0], mtu).unwrap();
        let mut receiver = HandBackReceiver::new(pieces[0].hash());
        let mut pending: Option<Vec<u8>> = None;
        let mut receipt: Option<(Vec<u8>, bool)> = None;
        let mut cancelled = false;
        let mut complete = false;
        for action in actions {
            if cancelled {
                prop_assert_eq!(sender.next_packet(), Err(TransferError::Cancelled));
                prop_assert_eq!(sender.retry_packet(), Err(TransferError::Cancelled));
                prop_assert_eq!(receiver.receive(&[]), Err(TransferError::Cancelled));
                prop_assert!(!sender.is_complete());
                continue;
            }
            match action {
                0 => {
                    let packet = sender.next_packet().unwrap();
                    if pending.is_some() || complete {
                        prop_assert_eq!(packet, None);
                    } else {
                        prop_assert!(packet.is_some());
                        pending = packet;
                    }
                }
                1 => if let Some(packet) = &pending {
                    let received = receiver.receive(packet).unwrap();
                    prop_assert!(received.piece().is_none_or(|piece| piece == &pieces[0]));
                    receipt = Some((received.acknowledgement().to_vec(), received.piece().is_some()));
                },
                2 => if let Some((ack, done)) = receipt.take() {
                    sender.acknowledge(&ack).unwrap();
                    pending = None;
                    complete = done;
                },
                3 => {
                    let expected = pending.clone().ok_or(TransferError::State);
                    prop_assert_eq!(sender.retry_packet(), expected);
                }
                4 => {
                    sender.cancel();
                    receiver.cancel();
                    cancelled = true;
                    complete = false;
                }
                _ => prop_assert_eq!(sender.acknowledge(&[]), Err(TransferError::Frame)),
            }
            prop_assert_eq!(sender.is_complete(), complete);
        }
    }
}
