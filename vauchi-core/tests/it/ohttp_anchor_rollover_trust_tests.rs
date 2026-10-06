// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Following a relay's anchor rollover (#288 plan 1.5, decision 0.13): the
//! client moves only to the backup it already committed to, so neither a
//! lost anchor strands it nor a stolen one diverts it.

use ed25519_dalek::{Signer, SigningKey};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use vauchi_core::network::ohttp_key_trust::{
    OhttpAnchor, RolloverRejection, accept_anchor_rollover, backup_commitment,
};
use vauchi_protocol::ohttp_key::{AnchorRollover, backup_commitment_message};

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn public(seed: u8) -> [u8; 32] {
    key(seed).verifying_key().to_bytes()
}

/// `backup` takes over and commits to `next_backup`, signing as itself.
fn rollover(backup: u8, next_backup: u8) -> AnchorRollover {
    let new_anchor = public(backup);
    let next_commitment = backup_commitment(&public(next_backup));
    AnchorRollover {
        new_anchor,
        next_commitment,
        signature: key(backup)
            .sign(&AnchorRollover::signing_message(
                &new_anchor,
                &next_commitment,
            ))
            .to_bytes(),
    }
}

/// Anchor 1 with backup 2 committed: what a client gets from its config.
fn held() -> OhttpAnchor {
    OhttpAnchor {
        anchor: public(1),
        backup_commitment: backup_commitment(&public(2)),
    }
}

// @internal
#[test]
fn the_commitment_is_sha256_of_the_domain_and_the_backup_key() {
    let expected: [u8; 32] = Sha256::digest(backup_commitment_message(&public(2))).into();

    assert_eq!(backup_commitment(&public(2)), expected);
    assert_ne!(backup_commitment(&public(2)), backup_commitment(&public(3)));
}

/// The lost-anchor case: anchor 1 signs nothing, its committed backup 2
/// takes over and commits to backup 3.
// @internal
#[test]
fn the_committed_backup_takes_over_and_names_the_next() {
    let result = accept_anchor_rollover(&held(), &[rollover(2, 3)]);

    assert_eq!(
        result,
        Ok(OhttpAnchor {
            anchor: public(2),
            backup_commitment: backup_commitment(&public(3)),
        })
    );
}

/// A client that missed two rollovers walks the chain to the end.
// @internal
#[test]
fn a_client_walks_every_rollover_it_missed() {
    let result = accept_anchor_rollover(&held(), &[rollover(2, 3), rollover(3, 4)]);

    assert_eq!(result.map(|state| state.anchor), Ok(public(3)));
}

/// The relay serves the chain from the start; a client already past the
/// first step applies only the rest.
// @internal
#[test]
fn records_the_client_already_followed_are_skipped() {
    let after_first = OhttpAnchor {
        anchor: public(2),
        backup_commitment: backup_commitment(&public(3)),
    };

    let result = accept_anchor_rollover(&after_first, &[rollover(2, 3), rollover(3, 4)]);

    assert_eq!(
        result,
        Ok(OhttpAnchor {
            anchor: public(3),
            backup_commitment: backup_commitment(&public(4)),
        })
    );
}

// @internal
#[test]
fn nothing_newer_leaves_the_anchor_as_held() {
    assert_eq!(accept_anchor_rollover(&held(), &[]), Ok(held()));
    let at_end = OhttpAnchor {
        anchor: public(2),
        backup_commitment: backup_commitment(&public(3)),
    };
    assert_eq!(
        accept_anchor_rollover(&at_end, &[rollover(2, 3)]),
        Ok(at_end.clone())
    );
}

/// The stolen-anchor case: a thief can sign anything as anchor 1, and can
/// mint a key of their own, but not one that hashes to the commitment.
// @internal
#[test]
fn a_successor_the_client_never_committed_to_is_refused() {
    let thiefs_key = rollover(9, 10);

    assert_eq!(
        accept_anchor_rollover(&held(), &[thiefs_key]),
        Err(RolloverRejection::NotTheCommittedBackup { step: 0 })
    );
}

/// The retired anchor's signature is worth nothing: only the backup itself
/// may sign its takeover.
// @internal
#[test]
fn a_rollover_signed_by_the_old_anchor_is_refused() {
    let mut signed_by_old = rollover(2, 3);
    signed_by_old.signature = key(1)
        .sign(&AnchorRollover::signing_message(
            &signed_by_old.new_anchor,
            &signed_by_old.next_commitment,
        ))
        .to_bytes();

    assert_eq!(
        accept_anchor_rollover(&held(), &[signed_by_old]),
        Err(RolloverRejection::BadSignature { step: 0 })
    );
}

/// An outer relay rewriting the next commitment would capture the step
/// after; the signature covers it.
// @internal
#[test]
fn a_rewritten_next_commitment_is_refused() {
    let mut rewritten = rollover(2, 3);
    rewritten.next_commitment = backup_commitment(&public(9));

    assert_eq!(
        accept_anchor_rollover(&held(), &[rewritten]),
        Err(RolloverRejection::BadSignature { step: 0 })
    );
}

/// One bad link refuses the whole chain: the client never stops halfway on
/// a state the relay did not mean.
// @internal
#[test]
fn a_bad_later_link_refuses_the_whole_chain() {
    let result = accept_anchor_rollover(&held(), &[rollover(2, 3), rollover(9, 10)]);

    assert_eq!(
        result,
        Err(RolloverRejection::NotTheCommittedBackup { step: 1 })
    );
}

proptest! {
    /// CC-04: flipping any bit of a valid rollover makes it unacceptable.
    // @internal
    #[test]
    fn any_bit_flip_in_a_rollover_is_refused(byte in 1usize..129, bit in 0u8..8) {
        let mut encoded = rollover(2, 3).encode();
        encoded[byte] ^= 1 << bit;
        let tampered = AnchorRollover::decode(&encoded).expect("same length decodes");

        prop_assert!(accept_anchor_rollover(&held(), &[tampered]).is_err());
    }
}
