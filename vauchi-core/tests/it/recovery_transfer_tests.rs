// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::recovery::transfer::{HandBackReceiver, HandBackSender};
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
