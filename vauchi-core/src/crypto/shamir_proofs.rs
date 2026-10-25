// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Kani proof for the Lagrange interpolation behind Shamir reconstruction
//! (#757).
//!
//! Run with `cargo kani -p vauchi-core -Z stubbing --harness crypto::shamir::proofs`.
//! Up to `MAX_POINTS` arbitrary points, for the host pointer width only.
//! CBMC stalls on bit-by-bit GF(256) multiplication, so `gf_mul` returns an
//! arbitrary byte here; the unit tests check it against an independent
//! reference for every pair of bytes and `gf_inv` for every non-zero byte.
//! `reconstruct` itself has no proof: its per-byte `Vec` of points gives no
//! verdict in 15 minutes even with the interpolation stubbed.

use super::{ShamirError, lagrange_interpolate_at_zero};

const MAX_POINTS: usize = 3;

fn any_field_element(_: u8, _: u8) -> u8 {
    kani::any()
}

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
