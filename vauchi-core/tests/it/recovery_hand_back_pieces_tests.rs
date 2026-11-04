// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The recovering phone takes each guardian's share from a QR code and the
//! backup in pieces over Bluetooth (vauchi/private#796, P33). It keeps
//! only pieces a share it took names, counts them until the backup is
//! whole, resumes after a restart, and drops a backup that does not match
//! its hash so it can be sent again.

use sha2::{Digest, Sha256};
use vauchi_core::sync::guardian_piece::{
    GuardianBackupPiece, HandBackShare, MAX_HAND_BACK_SHARE_BYTES, PieceAssembly,
    split_backup_for_hand_back,
};
use vauchi_core::{BackupProgress, Storage, SymmetricKey, Vauchi, VauchiConfig};

use crate::common::lost_phone::{Lost, alice_pk_hex, lost};

/// A share of Alice's backup, from the first guardian, sealed to the
/// phone's recovery key but naming `backup` instead of the backup it
/// opens: enough for the phone to take pieces of `backup`.
fn share_naming(lost: &Lost, claim: &str, backup: &[u8]) -> Vec<u8> {
    let guardian = lost.guardians[0].vauchi();
    let recovery_pk = *guardian
        .parse_recovery_claim_b64(claim)
        .unwrap()
        .new_pk()
        .as_bytes();
    let kept = guardian
        .storage()
        .guardian_backups()
        .kept(&alice_pk_hex(lost))
        .unwrap()
        .unwrap();
    let resealed = guardian
        .respond_to_recovery(kept.sealed_share(), &recovery_pk)
        .unwrap();
    HandBackShare::new(kept.created_at(), resealed, Sha256::digest(backup).into())
        .unwrap()
        .encode()
}

fn three_pieces() -> (Vec<u8>, Vec<Vec<u8>>) {
    let backup: Vec<u8> = (0..120_000).map(|i| (i * 13 % 251) as u8).collect();
    let pieces = split_backup_for_hand_back(&backup)
        .unwrap()
        .iter()
        .map(GuardianBackupPiece::encode)
        .collect();
    (backup, pieces)
}

fn progress(received: u16, count: u16) -> BackupProgress {
    BackupProgress { received, count }
}

// @internal
#[test]
fn the_share_fits_one_qr_code_and_names_the_backup_the_pieces_carry() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();

    let hand_back = lost.guardians[0]
        .vauchi()
        .hand_back_in_person(&claim)
        .unwrap();

    assert!(hand_back.share().len() <= MAX_HAND_BACK_SHARE_BYTES);
    let mut assembly = PieceAssembly::default();
    let mut whole = None;
    for piece in hand_back.backup_pieces() {
        whole = assembly
            .add(GuardianBackupPiece::decode(piece).unwrap())
            .unwrap()
            .or(whole);
    }
    let whole = whole.expect("the pieces make a whole backup");
    let share = HandBackShare::decode(hand_back.share()).unwrap();
    assert_eq!(
        share.backup_hash(),
        <[u8; 32]>::from(Sha256::digest(&whole))
    );
}

// @internal
#[test]
fn a_piece_no_taken_share_names_is_refused() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let hand_back = lost.guardians[0]
        .vauchi()
        .hand_back_in_person(&claim)
        .unwrap();
    let piece = &hand_back.backup_pieces()[0];

    assert!(phone.take_hand_back_piece(piece).is_err());

    phone.take_hand_back(hand_back.share()).unwrap();
    assert_eq!(phone.take_hand_back_piece(piece).unwrap(), progress(1, 1));
}

// @internal
#[test]
fn pieces_are_counted_until_the_backup_is_whole_and_a_resent_one_counts_once() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let (backup, pieces) = three_pieces();
    phone
        .take_hand_back(&share_naming(&lost, &claim, &backup))
        .unwrap();

    assert_eq!(
        phone.take_hand_back_piece(&pieces[2]).unwrap(),
        progress(1, 3)
    );
    assert_eq!(
        phone.take_hand_back_piece(&pieces[2]).unwrap(),
        progress(1, 3)
    );
    assert_eq!(
        phone.take_hand_back_piece(&pieces[0]).unwrap(),
        progress(2, 3)
    );
    let whole = phone.take_hand_back_piece(&pieces[1]).unwrap();
    assert_eq!(whole, progress(3, 3));
    assert!(whole.is_whole());
}

// @internal
#[test]
fn a_backup_that_does_not_match_its_hash_is_dropped_so_it_can_be_sent_again() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let (backup, pieces) = three_pieces();
    phone
        .take_hand_back(&share_naming(&lost, &claim, &backup))
        .unwrap();
    let mut tampered = pieces[0].clone();
    *tampered.last_mut().unwrap() ^= 1;

    phone.take_hand_back_piece(&tampered).unwrap();
    phone.take_hand_back_piece(&pieces[1]).unwrap();
    assert!(phone.take_hand_back_piece(&pieces[2]).is_err());

    assert_eq!(
        phone.take_hand_back_piece(&pieces[0]).unwrap(),
        progress(1, 3)
    );
    phone.take_hand_back_piece(&pieces[1]).unwrap();
    assert_eq!(
        phone.take_hand_back_piece(&pieces[2]).unwrap(),
        progress(3, 3)
    );
}

// @internal
#[test]
fn a_piece_giving_the_same_backup_another_count_is_refused() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let (backup, pieces) = three_pieces();
    phone
        .take_hand_back(&share_naming(&lost, &claim, &backup))
        .unwrap();
    phone.take_hand_back_piece(&pieces[0]).unwrap();
    let mut recounted = pieces[1].clone();
    recounted[16..18].copy_from_slice(&4u16.to_be_bytes());

    assert!(phone.take_hand_back_piece(&recounted).is_err());
    assert_eq!(
        phone.take_hand_back_piece(&pieces[1]).unwrap(),
        progress(2, 3)
    );
}

/// Every group's share and one guardian's backup restore Alice.
// @internal
#[test]
fn one_guardians_backup_with_a_share_of_every_group_restores_the_identity() {
    let lost = lost();
    let mut phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let hand_backs: Vec<_> = lost
        .guardians
        .iter()
        .map(|guardian| guardian.vauchi().hand_back_in_person(&claim).unwrap())
        .collect();
    for hand_back in &hand_backs {
        phone.take_hand_back(hand_back.share()).unwrap();
    }
    for piece in hand_backs[1].backup_pieces() {
        phone.take_hand_back_piece(piece).unwrap();
    }

    phone.restore_identity_from_hand_backs(None).unwrap();

    assert_eq!(
        phone.identity().unwrap().signing_public_key(),
        lost.alice.identity().unwrap().signing_public_key()
    );
}

/// The shares taken before a restart and the pieces taken after it
/// restore the identity: the ceremony survives the phone being closed.
// @internal
#[test]
fn a_recovery_interrupted_by_a_restart_resumes_where_it_stopped() {
    let lost = lost();
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let config = || {
        VauchiConfig::with_storage_path(dir.path().join("phone.db")).with_storage_key(key.clone())
    };
    let (backup, pieces) = three_pieces();
    let hand_back = {
        let phone = Vauchi::new(config()).unwrap();
        let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
        // The first guardian's share comes naming the three-piece backup.
        phone
            .take_hand_back(&share_naming(&lost, &claim, &backup))
            .unwrap();
        for guardian in &lost.guardians[1..] {
            let hand_back = guardian.vauchi().hand_back_in_person(&claim).unwrap();
            phone.take_hand_back(hand_back.share()).unwrap();
        }
        phone.take_hand_back_piece(&pieces[0]).unwrap();
        lost.guardians[2]
            .vauchi()
            .hand_back_in_person(&claim)
            .unwrap()
    };

    let mut phone = Vauchi::new(config()).unwrap();
    assert_eq!(
        phone.take_hand_back_piece(&pieces[1]).unwrap(),
        progress(2, 3)
    );
    for piece in hand_back.backup_pieces() {
        phone.take_hand_back_piece(piece).unwrap();
    }
    phone.restore_identity_from_hand_backs(None).unwrap();

    assert_eq!(
        phone.identity().unwrap().signing_public_key(),
        lost.alice.identity().unwrap().signing_public_key()
    );
}

// @internal
#[test]
fn the_backup_pieces_survive_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage
        .recovery()
        .keep_backup_piece(&[5; 32], 0, 2, b"piece", 7)
        .unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage.recovery().backup_pieces(&[5; 32]).unwrap(),
        vec![b"piece".to_vec()]
    );
}
