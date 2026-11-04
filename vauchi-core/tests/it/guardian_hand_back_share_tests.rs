// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! In the recovery meeting a guardian shows its share as a QR code and
//! sends the backup over Bluetooth in pieces (vauchi/private#796, P33).
//! The share names the backup's hash; the pieces of one backup are the
//! same bytes however often it is sent.

use proptest::prelude::*;
use sha2::{Digest, Sha256};
use vauchi_core::sync::guardian_piece::{
    GuardianBackupPiece, HandBackShare, MAX_HAND_BACK_SHARE_BYTES, PieceAssembly, PieceError,
    split_backup_for_hand_back,
};

fn backup(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 % 253) as u8).collect()
}

// @internal
#[test]
fn a_share_names_the_backup_its_pieces_carry() {
    let backup = backup(120_000);
    let hash: [u8; 32] = Sha256::digest(&backup).into();

    let share = HandBackShare::new(7, vec![1; 124], hash).unwrap();
    let mut assembly = PieceAssembly::default();
    let mut whole = None;
    for piece in split_backup_for_hand_back(&backup).unwrap() {
        assert_eq!(piece.hash(), share.backup_hash());
        whole = assembly.add(piece).unwrap().or(whole);
    }

    assert_eq!(whole, Some(backup));
}

// @internal
#[test]
fn a_backup_sent_twice_gives_the_same_pieces() {
    let backup = backup(120_000);

    let first: Vec<Vec<u8>> = split_backup_for_hand_back(&backup)
        .unwrap()
        .iter()
        .map(GuardianBackupPiece::encode)
        .collect();
    let again: Vec<Vec<u8>> = split_backup_for_hand_back(&backup)
        .unwrap()
        .iter()
        .map(GuardianBackupPiece::encode)
        .collect();

    assert_eq!(first.len(), 3);
    assert_eq!(first, again);
}

// @internal
#[test]
fn a_share_with_the_largest_sealed_share_fits_its_bound() {
    let share = HandBackShare::new(u64::MAX, vec![9; 1024], [3; 32]).unwrap();

    assert_eq!(share.encode().len(), MAX_HAND_BACK_SHARE_BYTES);
}

// @internal
#[test]
fn malformed_shares_are_refused() {
    let good = HandBackShare::new(5, vec![1; 124], [2; 32])
        .unwrap()
        .encode();
    let mut long_share = good.clone();
    long_share[8..10].copy_from_slice(&2000u16.to_be_bytes());
    let mut zero_share = 5u64.to_be_bytes().to_vec();
    zero_share.extend_from_slice(&0u16.to_be_bytes());
    zero_share.extend_from_slice(&[2; 32]);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("time only", good[..8].to_vec()),
        ("hash cut short", good[..good.len() - 1].to_vec()),
        ("trailing byte", [good.as_slice(), &[0]].concat()),
        ("share length past the end", long_share),
        ("empty share", zero_share),
    ];

    for (name, bytes) in cases {
        assert_eq!(
            HandBackShare::decode(&bytes),
            Err(PieceError::Malformed),
            "{name}"
        );
    }
    assert!(HandBackShare::new(5, vec![], [2; 32]).is_err());
    assert!(HandBackShare::new(5, vec![1; 1025], [2; 32]).is_err());
}

// @internal
#[test]
fn an_empty_backup_has_no_pieces() {
    assert_eq!(split_backup_for_hand_back(&[]), Err(PieceError::Size));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // @internal
    #[test]
    fn any_share_decodes_to_itself(
        created_at in any::<u64>(),
        sealed in proptest::collection::vec(any::<u8>(), 1..=1024),
        hash in any::<[u8; 32]>(),
    ) {
        let share = HandBackShare::new(created_at, sealed.clone(), hash).unwrap();

        let decoded = HandBackShare::decode(&share.encode()).unwrap();

        prop_assert_eq!(decoded.created_at(), created_at);
        prop_assert_eq!(decoded.sealed_share(), sealed.as_slice());
        prop_assert_eq!(decoded.backup_hash(), hash);
    }

    // @internal
    #[test]
    fn arbitrary_bytes_are_refused_or_decode_to_a_share_that_encodes_back(
        bytes in proptest::collection::vec(any::<u8>(), 0..200),
    ) {
        match HandBackShare::decode(&bytes) {
            Ok(share) => prop_assert_eq!(share.encode(), bytes),
            Err(error) => prop_assert_eq!(error, PieceError::Malformed),
        }
    }
}
