// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A guardian backup travels to its guardian in mailbox pieces of at most
//! 48 KB, reassembled in any order and checked against the whole
//! delivery's hash (vauchi/private#796, decision D9).

use proptest::prelude::*;
use vauchi_core::backup::{
    GUARDIAN_PIECE_MAX_BYTES, GuardianBackupPiece, MAX_GUARDIAN_DELIVERY_BYTES, PieceAssembly,
    PieceError, split_into_pieces,
};

const BACKUP: [u8; 16] = [9; 16];

fn delivery(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

fn reassemble(pieces: impl IntoIterator<Item = GuardianBackupPiece>) -> Option<Vec<u8>> {
    let mut assembly = PieceAssembly::default();
    let mut done = None;
    for piece in pieces {
        let decoded = GuardianBackupPiece::decode(&piece.encode()).unwrap();
        if let Some(whole) = assembly.add(decoded).unwrap() {
            done = Some(whole);
        }
    }
    done
}

// @internal
#[test]
fn a_two_megabyte_backup_arrives_whole_from_pieces_in_reverse_order() {
    let whole = delivery(2 * 1024 * 1024);
    let pieces = split_into_pieces(BACKUP, &whole).unwrap();

    assert_eq!(pieces.len(), 43);
    assert!(
        pieces
            .iter()
            .all(|piece| piece.encode().len() <= GUARDIAN_PIECE_MAX_BYTES)
    );
    assert_eq!(reassemble(pieces.into_iter().rev()), Some(whole));
}

// @internal
#[test]
fn a_backup_is_not_whole_while_a_piece_is_missing() {
    let pieces = split_into_pieces(BACKUP, &delivery(150_000)).unwrap();
    let missing_one = pieces.into_iter().skip(1);

    assert_eq!(reassemble(missing_one), None);
}

// @internal
#[test]
fn a_piece_received_twice_with_the_same_bytes_changes_nothing() {
    let whole = delivery(100_000);
    let pieces = split_into_pieces(BACKUP, &whole).unwrap();
    let mut assembly = PieceAssembly::default();

    assert_eq!(assembly.add(pieces[0].clone()), Ok(None));
    assert_eq!(assembly.add(pieces[0].clone()), Ok(None));
    assert_eq!(assembly.add(pieces[1].clone()), Ok(None));
    assert_eq!(assembly.add(pieces[2].clone()), Ok(Some(whole)));
}

// @internal
#[test]
fn a_second_copy_of_a_piece_with_other_bytes_is_refused() {
    let pieces = split_into_pieces(BACKUP, &delivery(100_000)).unwrap();
    let mut other = pieces[0].encode();
    let last = other.len() - 1;
    other[last] ^= 1;
    let mut assembly = PieceAssembly::default();
    assembly.add(pieces[0].clone()).unwrap();

    assert_eq!(
        assembly.add(GuardianBackupPiece::decode(&other).unwrap()),
        Err(PieceError::ConflictingDuplicate)
    );
}

// @internal
#[test]
fn a_backup_whose_bytes_do_not_match_its_hash_is_refused_when_complete() {
    let pieces = split_into_pieces(BACKUP, &delivery(100_000)).unwrap();
    let mut assembly = PieceAssembly::default();
    let mut tampered = pieces[1].encode();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;

    assert_eq!(assembly.add(pieces[0].clone()), Ok(None));
    assert_eq!(
        assembly.add(GuardianBackupPiece::decode(&tampered).unwrap()),
        Ok(None)
    );
    assert_eq!(
        assembly.add(pieces[2].clone()),
        Err(PieceError::HashMismatch)
    );
}

// @internal
#[test]
fn a_piece_of_another_backup_is_refused() {
    let ours = split_into_pieces(BACKUP, &delivery(100_000)).unwrap();
    let theirs = split_into_pieces([1; 16], &delivery(100_000)).unwrap();
    let mut assembly = PieceAssembly::default();
    assembly.add(ours[0].clone()).unwrap();

    assert_eq!(
        assembly.add(theirs[1].clone()),
        Err(PieceError::OtherBackup)
    );
}

// @internal
#[test]
fn an_empty_or_oversized_delivery_is_not_split() {
    assert_eq!(split_into_pieces(BACKUP, &[]), Err(PieceError::Size));
    assert_eq!(
        split_into_pieces(BACKUP, &vec![0; MAX_GUARDIAN_DELIVERY_BYTES + 1]),
        Err(PieceError::Size)
    );
    assert!(split_into_pieces(BACKUP, &vec![0; MAX_GUARDIAN_DELIVERY_BYTES]).is_ok());
}

/// Each case is a valid piece of a three-piece backup with one header
/// field or the body bent out of shape. The header is the backup id (16
/// bytes), count and index (2 bytes each, big-endian) and the hash (32).
// @internal
#[test]
fn a_malformed_piece_is_refused_at_decode() {
    let pieces = split_into_pieces(BACKUP, &delivery(100_000)).unwrap();
    let first = pieces[0].encode();

    let with_count = |count: u16| {
        let mut bytes = first.clone();
        bytes[16..18].copy_from_slice(&count.to_be_bytes());
        bytes
    };
    let with_index = |index: u16| {
        let mut bytes = first.clone();
        bytes[18..20].copy_from_slice(&index.to_be_bytes());
        bytes
    };
    let short_inner = {
        let mut bytes = first.clone();
        bytes.truncate(bytes.len() - 1);
        bytes
    };
    let oversized_last = {
        let mut bytes = pieces[2].encode();
        bytes.resize(GUARDIAN_PIECE_MAX_BYTES + 1, 0);
        bytes
    };

    let cases: [(&str, Vec<u8>); 8] = [
        ("empty", Vec::new()),
        ("header only part", first[..30].to_vec()),
        ("no body", first[..52].to_vec()),
        ("count zero", with_count(0)),
        ("count above the cap", with_count(u16::MAX)),
        ("index past the count", with_index(3)),
        ("a piece before the last one cut short", short_inner),
        ("a piece longer than the limit", oversized_last),
    ];
    for (name, bytes) in cases {
        assert!(
            GuardianBackupPiece::decode(&bytes).is_err(),
            "{name} decoded"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // @internal
    #[test]
    fn any_delivery_arrives_whole_from_its_pieces_in_any_order(
        len in 1usize..(4 * GUARDIAN_PIECE_MAX_BYTES),
        seed in any::<u64>(),
        backup in any::<[u8; 16]>(),
    ) {
        let whole: Vec<u8> = (0..len).map(|i| (i as u64 ^ seed) as u8).collect();
        let mut pieces = split_into_pieces(backup, &whole).unwrap();
        let rotate = (seed as usize) % pieces.len();
        pieces.rotate_left(rotate);
        if seed % 2 == 0 {
            pieces.reverse();
        }

        prop_assert_eq!(reassemble(pieces), Some(whole));
    }

    // @internal
    #[test]
    fn arbitrary_bytes_never_panic_at_decode(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let _ = GuardianBackupPiece::decode(&bytes);
    }
}
