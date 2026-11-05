// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::sync::guardian_piece::{GuardianBackupPiece, HandBackShare};
use vauchi_core::sync::guardian_transfer::{HandBackReceiver, HandBackSender};
use vauchi_core::{Vauchi, VauchiError};

use crate::common::lost_phone::{alice_pk_hex, lost};

fn refused(error: VauchiError) {
    assert!(matches!(error, VauchiError::InvalidState(message)
        if message == "Recovery transfer could not continue"));
}

// @internal — P33: the durable ACK boundary, not the assembled-app ceremony.
#[test]
fn recovery_transfer_final_ack_follows_storage_and_same_identity_restore() {
    let lost = lost();
    let mut phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    for guardian in &lost.guardians {
        let hand_back = guardian.vauchi().hand_back_in_person(&claim).unwrap();
        phone.take_hand_back(hand_back.share()).unwrap();
        if guardian.id_at_sharer != lost.guardians[0].id_at_sharer {
            continue;
        }
        let hash = HandBackShare::decode(hand_back.share())
            .unwrap()
            .backup_hash();
        let mut receiver = HandBackReceiver::new(hash);
        for encoded in hand_back.backup_pieces() {
            let piece = GuardianBackupPiece::decode(encoded).unwrap();
            let mut sender = HandBackSender::new(&piece, 23).unwrap();
            while !sender.is_complete() {
                let packet = sender.next_packet().unwrap().unwrap();
                assert!(packet.len() <= 20);
                let ack = phone.take_hand_back_frame(&mut receiver, &packet).unwrap();
                sender.acknowledge(&ack).unwrap();
            }
            assert_eq!(
                phone.storage().recovery().backup_pieces(&hash).unwrap(),
                hand_back.backup_pieces()[..=usize::from(piece.index())]
            );
        }
    }
    assert_eq!(phone.restore_identity_from_hand_backs(None).unwrap(), 4);
    assert_eq!(
        phone.identity().unwrap().signing_public_key(),
        lost.alice.identity().unwrap().signing_public_key()
    );
}

// @internal — P33: an unsuccessful durable write must not release the final ACK.
#[test]
fn recovery_transfer_storage_failure_withholds_final_ack_and_allows_retry() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let hand_back = lost.guardians[0]
        .vauchi()
        .hand_back_in_person(&claim)
        .unwrap();
    phone.take_hand_back(hand_back.share()).unwrap();
    let piece = GuardianBackupPiece::decode(&hand_back.backup_pieces()[0]).unwrap();
    let mut receiver = HandBackReceiver::new(piece.hash());
    let mut sender = HandBackSender::new(&piece, 23).unwrap();
    phone
        .storage()
        .connection()
        .execute_batch("PRAGMA query_only = ON")
        .unwrap();

    let final_packet = loop {
        let packet = sender.next_packet().unwrap().unwrap();
        match phone.take_hand_back_frame(&mut receiver, &packet) {
            Ok(ack) => sender.acknowledge(&ack).unwrap(),
            Err(error) => {
                refused(error);
                break packet;
            }
        }
    };
    assert!(!sender.is_complete());
    assert_eq!(sender.retry_packet().unwrap(), final_packet);
    assert_eq!(
        phone
            .storage()
            .recovery()
            .backup_piece_progress(&piece.hash())
            .unwrap(),
        None
    );
    phone
        .storage()
        .connection()
        .execute_batch("PRAGMA query_only = OFF")
        .unwrap();

    let ack = phone
        .take_hand_back_frame(&mut receiver, &final_packet)
        .unwrap();
    assert_eq!(
        phone
            .storage()
            .recovery()
            .backup_pieces(&piece.hash())
            .unwrap(),
        vec![piece.encode()]
    );
    sender.acknowledge(&ack).unwrap();
    assert!(sender.is_complete());
    assert_eq!(
        phone
            .take_hand_back_frame(&mut receiver, &final_packet)
            .unwrap(),
        ack
    );
    assert_eq!(
        phone
            .storage()
            .recovery()
            .backup_piece_progress(&piece.hash())
            .unwrap(),
        Some((1, 1))
    );
}

// @internal — DC-01/DC-02: no ACK before an accepted share names the backup.
#[test]
fn recovery_transfer_unaccepted_backup_is_refused_before_acknowledging_any_packet() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let hand_back = lost.guardians[0]
        .vauchi()
        .hand_back_in_person(&claim)
        .unwrap();
    let piece = GuardianBackupPiece::decode(&hand_back.backup_pieces()[0]).unwrap();
    let mut receiver = HandBackReceiver::new(piece.hash());
    let mut sender = HandBackSender::new(&piece, 23).unwrap();
    let first = sender.next_packet().unwrap().unwrap();
    refused(
        phone
            .take_hand_back_frame(&mut receiver, &first)
            .unwrap_err(),
    );
    phone.take_hand_back(hand_back.share()).unwrap();
    assert_eq!(
        phone.take_hand_back_frame(&mut receiver, &first).unwrap(),
        b"RBA1\0\0\0\x0a"
    );
}

// @internal — DC-02: cancellation and a changed recovery target reject late bytes.
#[test]
fn recovery_transfer_cancelled_or_obsolete_receivers_cannot_acknowledge_late_packets() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let hand_back = lost.guardians[0]
        .vauchi()
        .hand_back_in_person(&claim)
        .unwrap();
    phone.take_hand_back(hand_back.share()).unwrap();
    let piece = GuardianBackupPiece::decode(&hand_back.backup_pieces()[0]).unwrap();
    let mut sender = HandBackSender::new(&piece, 23).unwrap();
    let first = sender.next_packet().unwrap().unwrap();
    let mut cancelled = HandBackReceiver::new(piece.hash());
    cancelled.cancel();
    refused(
        phone
            .take_hand_back_frame(&mut cancelled, &first)
            .unwrap_err(),
    );
    let mut obsolete = HandBackReceiver::new(piece.hash());
    phone.begin_recovery(&"ab".repeat(32)).unwrap();
    refused(
        phone
            .take_hand_back_frame(&mut obsolete, &first)
            .unwrap_err(),
    );
    assert_eq!(
        phone
            .storage()
            .recovery()
            .backup_piece_progress(&piece.hash())
            .unwrap(),
        None
    );
}
