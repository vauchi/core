// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Kani proofs for Shamir reconstruction from shares guardians return
//! (#757).
//!
//! Run with `cargo kani -p vauchi-core -Z stubbing --harness crypto::shamir::proofs`.
//! For the host pointer width only. The work is split so each proof stays
//! within CBMC's reach; what each stubs is proved or tested elsewhere:
//! - the interpolation, on arbitrary points, with `gf_mul` arbitrary (the
//!   unit tests check `gf_mul` against an independent reference for every
//!   pair of bytes and `gf_inv` for every non-zero byte);
//! - one byte's recovery (`recover_byte`), with the interpolation stubbed;
//! - the refusal of a tenth share.
//!
//! `reconstruct`'s threshold, count and duplicate checks over arbitrary
//! share indices have no proof: with `recover_byte` stubbed, CBMC still ran
//! past 8 GB, with a 256-entry table or pairwise comparison alike, and with
//! index values limited to 0–4; with fixed indices it verified in 38 s. The
//! unit tests cover those checks.
//!
//! `wipe_points` and the shares' wipe on drop are kept out: `zeroize`'s
//! barrier is inline assembly Kani cannot model. The wipe changes no
//! result; `wipe_points_clears_every_index_and_value` and
//! `tests/wipe_on_drop_tests.rs` check it.

use core::mem::ManuallyDrop;

use super::{
    MAX_COUNT, ShamirError, Share, lagrange_interpolate_at_zero, reconstruct, recover_byte,
};

const MAX_POINTS: usize = 3;

fn any_field_element(_: u8, _: u8) -> u8 {
    kani::any()
}

fn any_interpolation(_: &[(u8, u8)]) -> Result<u8, ShamirError> {
    if kani::any() {
        Ok(kani::any())
    } else {
        Err(ShamirError::DivisionByZero)
    }
}

fn no_wipe(_: &mut [(u8, u8)]) {}

fn xs_are_nonzero_and_distinct(xs: &[u8]) -> bool {
    xs.iter()
        .enumerate()
        .all(|(i, x)| *x != 0 && !xs[..i].contains(x))
}

#[kani::proof]
#[kani::unwind(5)]
#[kani::stub(super::gf_mul, any_field_element)]
fn interpolation_never_panics_and_refuses_a_zero_or_repeated_x() {
    let points: [(u8, u8); MAX_POINTS] = kani::any();
    let count: usize = kani::any_where(|count| *count <= MAX_POINTS);
    let points = &points[..count];
    let xs: [u8; MAX_POINTS] = core::array::from_fn(|i| points.get(i).map_or(0, |p| p.0));
    let xs = &xs[..count];

    match lagrange_interpolate_at_zero(points) {
        Ok(_) => assert!(xs_are_nonzero_and_distinct(xs)),
        Err(ShamirError::InvalidShareIndex) => assert!(xs.contains(&0)),
        Err(ShamirError::DuplicateIndices) => assert!(!xs_are_nonzero_and_distinct(xs)),
        // Reachable only because gf_mul is arbitrary here: with the real
        // field, distinct non-zero x values give a non-zero denominator.
        Err(ShamirError::DivisionByZero) => {}
        Err(_) => panic!("interpolation returned an error it never builds"),
    }
}

/// One byte's recovery never panics, for any byte position and up to one
/// share past the cap, and refuses more shares than the buffer holds.
#[kani::proof]
#[kani::unwind(12)]
#[kani::stub(super::lagrange_interpolate_at_zero, any_interpolation)]
#[kani::stub(super::wipe_points, no_wipe)]
fn recover_byte_never_panics() {
    let indices: [u8; MAX_COUNT as usize + 1] = kani::any();
    let all = ManuallyDrop::new(indices.map(|index| Share {
        index,
        value: [0; 32],
    }));
    let count: usize = kani::any_where(|count| *count <= MAX_COUNT as usize + 1);
    let byte_pos: usize = kani::any_where(|pos| *pos < 32);
    let mut points: [(u8, u8); MAX_COUNT as usize] = kani::any();

    let result = recover_byte(&all[..count], byte_pos, &mut points);

    if count > usize::from(MAX_COUNT) {
        assert_eq!(result, Err(ShamirError::CountTooHigh(MAX_COUNT)));
    }
}

/// A tenth share can only come from someone other than a guardian
/// (vauchi/private#816), whatever the threshold.
#[kani::proof]
#[kani::unwind(34)]
#[kani::stub(super::lagrange_interpolate_at_zero, any_interpolation)]
#[kani::stub(super::wipe_points, no_wipe)]
fn reconstruct_refuses_more_shares_than_guardians() {
    let shares = ManuallyDrop::new(core::array::from_fn::<_, { MAX_COUNT as usize + 1 }, _>(
        |i| Share {
            index: u8::try_from(i + 1).expect("at most 10"),
            value: [0; 32],
        },
    ));
    let threshold: u8 = kani::any();

    let error = reconstruct(&shares[..], threshold).unwrap_err();

    // A threshold outside 2..=8 is refused first; any other sees ten shares.
    assert!(matches!(
        error,
        ShamirError::ThresholdTooLow(_)
            | ShamirError::ThresholdTooHigh(_)
            | ShamirError::CountTooHigh(10)
    ));
}
