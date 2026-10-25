// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Kani proofs for Shamir reconstruction from shares guardians return
//! (#757).
//!
//! Run with `cargo kani -p vauchi-core -Z stubbing --harness crypto::shamir::proofs`.
//! For the host pointer width only. Three things are stubbed, each checked
//! elsewhere:
//! - `gf_mul` returns an arbitrary byte: CBMC stalls on bit-by-bit GF(256)
//!   multiplication, and the unit tests check it against an independent
//!   reference for every pair of bytes and `gf_inv` for every non-zero byte;
//! - `reconstruct`'s proofs stub the interpolation, which the first proof
//!   covers on arbitrary points, so share values are fixed;
//! - `wipe_points` and the shares' wipe on drop: `zeroize`'s barrier is
//!   inline assembly Kani cannot model. The wipe changes no result;
//!   `wipe_points_clears_every_index_and_value` checks it.
//!
//! A harness over arbitrary share indices and thresholds gives no verdict:
//! CBMC ran past 8 GB with four shares and with three. `reconstruct`'s
//! threshold, count and duplicate checks are covered by the unit tests.

use core::mem::ManuallyDrop;

use super::{MAX_COUNT, ShamirError, Share, lagrange_interpolate_at_zero, reconstruct};

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

    assert!(reconstruct(&shares[..], threshold).is_err());
}
