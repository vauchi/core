// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::recovery::transfer::{HandBackReceiver, HandBackSender, TransferError};
use vauchi_core::sync::guardian_piece::{GUARDIAN_PIECE_MAX_BYTES, split_backup_for_hand_back};

// @internal — P33: transport primitives do not complete the domain acceptance scenario.
#[test]
fn a_full_backup_piece_crosses_the_minimum_ble_mtu_without_a_command_burst() {
    let pieces = split_backup_for_hand_back(&vec![7; 60_000]).unwrap();
    let piece = &pieces[0];
    assert_eq!(piece.encode().len(), GUARDIAN_PIECE_MAX_BYTES);
    let mut sender = HandBackSender::new(piece, 23).unwrap();
    let mut receiver = HandBackReceiver::new(piece.hash());
    let mut completed = None;
    let mut packets = 0;

    while !sender.is_complete() {
        let packet = sender.next_packet().unwrap().unwrap();
        assert!(packet.len() <= 20);
        assert_eq!(sender.next_packet().unwrap(), None, "wait for the peer");
        let receipt = receiver.receive(&packet).unwrap();
        completed = receipt.piece().cloned().or(completed);
        sender.acknowledge(receipt.acknowledgement()).unwrap();
        packets += 1;
        assert!(packets < 10_000, "the transfer must finish");
    }

    assert_eq!(completed.as_ref(), Some(piece));
    assert!(
        packets > 256,
        "the old chunk-count cap must not block recovery"
    );
    assert_eq!(sender.next_packet().unwrap(), None);
}

// @internal
#[test]
fn a_lost_final_ack_can_be_retried_and_duplicate_acks_do_not_advance() {
    let pieces = split_backup_for_hand_back(&[7; 30]).unwrap();
    let piece = &pieces[0];
    let mut sender = HandBackSender::new(piece, 247).unwrap();
    let mut receiver = HandBackReceiver::new(piece.hash());
    let packet = sender.next_packet().unwrap().unwrap();
    let first = receiver.receive(&packet).unwrap();
    assert_eq!(first.piece(), Some(piece));
    assert!(!sender.is_complete());

    assert_eq!(sender.retry_packet().unwrap(), packet);
    let duplicate = receiver.receive(&packet).unwrap();
    assert_eq!(
        duplicate, first,
        "persistence can be attempted again before ACK"
    );
    sender.acknowledge(duplicate.acknowledgement()).unwrap();
    sender.acknowledge(first.acknowledgement()).unwrap();
    assert!(sender.is_complete());
    assert_eq!(sender.next_packet().unwrap(), None);
    assert_eq!(sender.retry_packet(), Err(TransferError::State));
}

// @internal — DC-01/DC-02: a forged offset or another piece cannot skip bytes.
#[test]
fn malformed_or_unexpected_acks_leave_the_packet_outstanding() {
    let pieces = split_backup_for_hand_back(&[7; 100]).unwrap();
    let mut sender = HandBackSender::new(&pieces[0], 23).unwrap();
    let mut receiver = HandBackReceiver::new(pieces[0].hash());
    let packet = sender.next_packet().unwrap().unwrap();
    let receipt = receiver.receive(&packet).unwrap();
    let good = receipt.acknowledgement().to_vec();
    let mut other_piece = good.clone();
    other_piece[4..6].copy_from_slice(&1u16.to_be_bytes());
    let mut unexpected_end = good.clone();
    unexpected_end[6..8].copy_from_slice(&11u16.to_be_bytes());
    let cases = [
        (vec![], TransferError::Frame),
        (good[..7].to_vec(), TransferError::Frame),
        ([good.as_slice(), &[0]].concat(), TransferError::Frame),
        (other_piece, TransferError::State),
        (unexpected_end, TransferError::State),
    ];

    for (ack, error) in cases {
        assert_eq!(sender.acknowledge(&ack), Err(error));
        assert_eq!(sender.next_packet().unwrap(), None);
        assert_eq!(sender.retry_packet().unwrap(), packet);
    }
    sender.acknowledge(&good).unwrap();
    assert!(sender.next_packet().unwrap().is_some());
}

// @internal — DC-01: size and offset bounds precede allocation or append.
#[test]
fn malformed_packets_are_refused_without_consuming_the_first_fragment() {
    let pieces = split_backup_for_hand_back(&[7; 100]).unwrap();
    let mut sender = HandBackSender::new(&pieces[0], 23).unwrap();
    let good = sender.next_packet().unwrap().unwrap();
    let mut zero = good.clone();
    zero[8..10].copy_from_slice(&0u16.to_be_bytes());
    let mut oversized = good.clone();
    oversized[8..10].copy_from_slice(&49_153u16.to_be_bytes());
    let mut overrun = good.clone();
    overrun[8..10].copy_from_slice(&1u16.to_be_bytes());
    let mut gap = good.clone();
    gap[6..8].copy_from_slice(&1u16.to_be_bytes());
    let mut wrong_magic = good.clone();
    wrong_magic[0] ^= 1;
    let mut oversized_attribute = good.clone();
    oversized_attribute[8..10].copy_from_slice(&1000u16.to_be_bytes());
    oversized_attribute.resize(513, 7);
    let cases = [
        (vec![], TransferError::Frame),
        (good[..10].to_vec(), TransferError::Frame),
        (oversized_attribute, TransferError::Frame),
        (wrong_magic, TransferError::Frame),
        (zero, TransferError::Frame),
        (oversized, TransferError::Frame),
        (overrun, TransferError::Frame),
        (gap, TransferError::State),
    ];
    for (bytes, error) in cases {
        let mut receiver = HandBackReceiver::new(pieces[0].hash());
        assert_eq!(receiver.receive(&bytes), Err(error));
        assert_eq!(receiver.receive(&good).unwrap().piece(), None);
    }
}

// @internal
#[test]
fn conflicting_duplicates_and_gaps_do_not_change_received_bytes() {
    let pieces = split_backup_for_hand_back(&[7; 100]).unwrap();
    let mut sender = HandBackSender::new(&pieces[0], 23).unwrap();
    let mut receiver = HandBackReceiver::new(pieces[0].hash());
    let first = sender.next_packet().unwrap().unwrap();
    let receipt = receiver.receive(&first).unwrap();
    assert_eq!(receiver.receive(&first).unwrap(), receipt);
    let mut changed_length = first.clone();
    changed_length[8..10].copy_from_slice(&153u16.to_be_bytes());
    assert_eq!(receiver.receive(&changed_length), Err(TransferError::State));
    let mut changed = first.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert_eq!(receiver.receive(&changed), Err(TransferError::State));
    sender.acknowledge(receipt.acknowledgement()).unwrap();
    let second = sender.next_packet().unwrap().unwrap();
    let mut gap = second.clone();
    gap[6..8].copy_from_slice(&11u16.to_be_bytes());
    assert_eq!(receiver.receive(&gap), Err(TransferError::State));
    let second_receipt = receiver.receive(&second).unwrap();
    sender
        .acknowledge(second_receipt.acknowledgement())
        .unwrap();
    sender.acknowledge(receipt.acknowledgement()).unwrap();
    assert_eq!(
        sender.next_packet().unwrap().unwrap()[6..8],
        20u16.to_be_bytes()
    );
}

// @internal
#[test]
fn a_completed_piece_must_match_the_accepted_hash_and_packet_index() {
    let pieces = split_backup_for_hand_back(&[7; 30]).unwrap();
    let piece = &pieces[0];
    let mut sender = HandBackSender::new(piece, 247).unwrap();
    let packet = sender.next_packet().unwrap().unwrap();
    let mut wrong_share = HandBackReceiver::new([9; 32]);
    assert_eq!(
        wrong_share.receive(&packet),
        Err(TransferError::PieceMismatch)
    );
    let mut wrong_index = packet.clone();
    wrong_index[4..6].copy_from_slice(&1u16.to_be_bytes());
    let mut receiver = HandBackReceiver::new(piece.hash());
    assert_eq!(
        receiver.receive(&wrong_index),
        Err(TransferError::PieceMismatch)
    );
    assert_eq!(receiver.receive(&packet).unwrap().piece(), Some(piece));
}

// @internal
#[test]
fn cancelling_discards_the_packet_and_refuses_late_data_or_acknowledgements() {
    let pieces = split_backup_for_hand_back(&[7; 30]).unwrap();
    let mut sender = HandBackSender::new(&pieces[0], 247).unwrap();
    let mut receiver = HandBackReceiver::new(pieces[0].hash());
    let packet = sender.next_packet().unwrap().unwrap();
    let receipt = receiver.receive(&packet).unwrap();

    sender.cancel();
    receiver.cancel();

    assert!(!sender.is_complete());
    assert_eq!(sender.next_packet(), Err(TransferError::Cancelled));
    assert_eq!(sender.retry_packet(), Err(TransferError::Cancelled));
    assert_eq!(
        sender.acknowledge(receipt.acknowledgement()),
        Err(TransferError::Cancelled)
    );
    assert_eq!(receiver.receive(&packet), Err(TransferError::Cancelled));
}

// @internal
#[test]
fn unusable_or_out_of_range_mtus_are_refused() {
    let pieces = split_backup_for_hand_back(&[7; 30]).unwrap();
    for mtu in [0, 1, 22, 518, u16::MAX] {
        assert_eq!(
            HandBackSender::new(&pieces[0], mtu).err(),
            Some(TransferError::Frame)
        );
    }
}

// @internal — only a final-fragment retry may request persistence again (A11).
#[test]
fn replaying_an_earlier_fragment_after_completion_does_not_repeat_the_whole_piece() {
    let pieces = split_backup_for_hand_back(&[7; 100]).unwrap();
    let piece = &pieces[0];
    let mut sender = HandBackSender::new(piece, 23).unwrap();
    let mut receiver = HandBackReceiver::new(piece.hash());
    let first = sender.next_packet().unwrap().unwrap();
    let first_receipt = receiver.receive(&first).unwrap();
    sender.acknowledge(first_receipt.acknowledgement()).unwrap();
    let mut last = Vec::new();
    let mut packets = 1;
    while !sender.is_complete() {
        packets += 1;
        assert!(packets < 1000);
        last = sender.next_packet().unwrap().unwrap();
        let receipt = receiver.receive(&last).unwrap();
        sender.acknowledge(receipt.acknowledgement()).unwrap();
    }

    let replay = receiver.receive(&first).unwrap();
    assert_eq!(replay.acknowledgement(), first_receipt.acknowledgement());
    assert_eq!(replay.piece(), None);
    assert_eq!(receiver.receive(&last).unwrap().piece(), Some(piece));
}

// @internal
#[test]
fn unsolicited_acknowledgements_cannot_skip_the_first_packet() {
    let pieces = split_backup_for_hand_back(&[7; 100]).unwrap();
    let piece = &pieces[0];
    let mut sender = HandBackSender::new(piece, 23).unwrap();
    let mut peer_sender = HandBackSender::new(piece, 23).unwrap();
    let mut receiver = HandBackReceiver::new(piece.hash());
    let first = peer_sender.next_packet().unwrap().unwrap();
    let receipt = receiver.receive(&first).unwrap();

    assert_eq!(sender.retry_packet(), Err(TransferError::State));
    assert_eq!(
        sender.acknowledge(receipt.acknowledgement()),
        Err(TransferError::State)
    );
    assert!(!sender.is_complete());
    assert_eq!(sender.next_packet().unwrap(), Some(first));
}

// @internal
#[test]
fn successive_pieces_share_a_receiver_without_mixing_their_buffers() {
    let pieces = split_backup_for_hand_back(&vec![7; 60_000]).unwrap();
    let mut receiver = HandBackReceiver::new(pieces[0].hash());
    let mut received = Vec::new();
    for piece in &pieces {
        let mut sender = HandBackSender::new(piece, 517).unwrap();
        let max_packets = piece.encode().len();
        let mut packets = 0;
        while !sender.is_complete() {
            packets += 1;
            assert!(packets <= max_packets);
            let packet = sender.next_packet().unwrap().unwrap();
            assert!(packet.len() <= 512);
            let receipt = receiver.receive(&packet).unwrap();
            if let Some(complete) = receipt.piece() {
                received.push(complete.clone());
            }
            sender.acknowledge(receipt.acknowledgement()).unwrap();
        }
    }
    assert_eq!(received, pieces);
}
