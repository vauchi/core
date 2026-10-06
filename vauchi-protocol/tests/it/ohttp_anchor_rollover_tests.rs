// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The anchor rollover record (#288 plan 1.5, decision 0.13): the wire
//! codec and the exact bytes the new anchor signs and the commitment hashes.

use proptest::prelude::*;
use vauchi_protocol::ohttp_key::{
    AnchorRollover, MAX_ROLLOVER_CHAIN, ROLLOVER_BYTES, ROLLOVER_VERSION, RolloverError,
    backup_commitment_message, decode_rollover_chain, encode_rollover_chain,
};

fn sample(seed: u8) -> AnchorRollover {
    AnchorRollover {
        new_anchor: [seed; 32],
        next_commitment: [seed.wrapping_add(1); 32],
        signature: [seed.wrapping_add(2); 64],
    }
}

// @internal
#[test]
fn a_rollover_is_129_bytes_version_first() {
    let encoded = sample(0x10).encode();

    assert_eq!(ROLLOVER_BYTES, 129);
    assert_eq!(encoded.len(), ROLLOVER_BYTES);
    assert_eq!(encoded[0], ROLLOVER_VERSION);
    assert_eq!(&encoded[1..33], &[0x10; 32]);
    assert_eq!(&encoded[33..65], &[0x11; 32]);
    assert_eq!(&encoded[65..], &[0x12; 64]);
}

// @internal
#[test]
fn a_rollover_round_trips() {
    let rollover = sample(0x10);

    assert_eq!(AnchorRollover::decode(&rollover.encode()), Ok(rollover));
}

/// Domain separation keeps a rollover signature from being replayed as any
/// other signature the anchor makes (ADR-007).
// @internal
#[test]
fn the_new_anchor_signs_domain_new_anchor_and_next_commitment() {
    let message = AnchorRollover::signing_message(&[0x10; 32], &[0x11; 32]);

    let domain = b"vauchi-ohttp-anchor-rollover-v1";
    assert_eq!(&message[..domain.len()], domain);
    assert_eq!(&message[domain.len()..domain.len() + 32], &[0x10; 32]);
    assert_eq!(&message[domain.len() + 32..], &[0x11; 32]);
}

// @internal
#[test]
fn the_commitment_hashes_its_own_domain_and_the_backup_key() {
    let message = backup_commitment_message(&[0x42; 32]);

    let domain = b"vauchi-ohttp-backup-anchor-v1";
    assert_eq!(&message[..domain.len()], domain);
    assert_eq!(&message[domain.len()..], &[0x42; 32]);
}

/// CC-14: whatever is not exactly one current-version record is refused.
// @internal
#[test]
fn anything_but_one_rollover_is_refused() {
    let good = sample(0x10).encode();
    let mut unknown_version = good;
    unknown_version[0] = 9;
    let mut trailing = good.to_vec();
    trailing.push(0);

    assert_eq!(AnchorRollover::decode(&[]), Err(RolloverError::Truncated));
    assert_eq!(
        AnchorRollover::decode(&good[..ROLLOVER_BYTES - 1]),
        Err(RolloverError::Truncated)
    );
    assert_eq!(
        AnchorRollover::decode(&trailing),
        Err(RolloverError::TrailingBytes)
    );
    assert_eq!(
        AnchorRollover::decode(&unknown_version),
        Err(RolloverError::UnknownVersion(9))
    );
}

/// A client that missed several rollovers walks the whole chain, in order.
// @internal
#[test]
fn a_chain_round_trips_in_order() {
    let chain = vec![sample(0x10), sample(0x20), sample(0x30)];

    let encoded = encode_rollover_chain(&chain);

    assert_eq!(encoded[0], 3);
    assert_eq!(encoded.len(), 1 + 3 * ROLLOVER_BYTES);
    assert_eq!(decode_rollover_chain(&encoded), Ok(chain));
}

// @internal
#[test]
fn a_chain_is_bounded_and_never_empty() {
    let too_long = vec![sample(0x10); MAX_ROLLOVER_CHAIN + 1];
    let mut overlong_count = encode_rollover_chain(&[sample(0x10)]);
    overlong_count[0] = 2;

    assert_eq!(decode_rollover_chain(&[0]), Err(RolloverError::EmptyChain));
    assert_eq!(decode_rollover_chain(&[]), Err(RolloverError::Truncated));
    assert_eq!(
        decode_rollover_chain(&encode_rollover_chain(&too_long)),
        Err(RolloverError::ChainTooLong(MAX_ROLLOVER_CHAIN + 1))
    );
    assert_eq!(
        decode_rollover_chain(&overlong_count),
        Err(RolloverError::Truncated)
    );
}

proptest! {
    // @internal
    #[test]
    fn any_rollover_round_trips(
        new_anchor in any::<[u8; 32]>(),
        next_commitment in any::<[u8; 32]>(),
        sig_a in any::<[u8; 32]>(),
        sig_b in any::<[u8; 32]>(),
    ) {
        let mut signature = [0u8; 64];
        signature[..32].copy_from_slice(&sig_a);
        signature[32..].copy_from_slice(&sig_b);
        let rollover = AnchorRollover { new_anchor, next_commitment, signature };

        prop_assert_eq!(AnchorRollover::decode(&rollover.encode()), Ok(rollover));
    }

    /// Arbitrary bytes never panic the chain decoder (DC-01), and whatever
    /// it accepts is canonical: it encodes back to exactly those bytes.
    // @internal
    #[test]
    fn whatever_the_chain_decoder_accepts_is_canonical(
        bytes in proptest::collection::vec(any::<u8>(), 0..600),
    ) {
        if let Ok(chain) = decode_rollover_chain(&bytes) {
            prop_assert_eq!(encode_rollover_chain(&chain), bytes);
        }
    }
}
