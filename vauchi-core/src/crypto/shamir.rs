// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shamir's Secret Sharing over GF(256).
//!
//! Splits a 32-byte secret into `n` shares such that any `t` shares can
//! reconstruct the secret, but fewer than `t` shares reveal no information.
//!
//! Uses GF(256) with the irreducible polynomial `x^8 + x^4 + x^3 + x + 1`
//! (0x11b), the same field as AES. Addition is XOR; multiplication is
//! carryless multiplication modulo 0x11b.
//!
//! # Security properties
//!
//! - Information-theoretic security: fewer than `t` shares reveal *nothing*.
//! - Each share is 33 bytes: 1-byte index + 32-byte value.
//! - The secret is the polynomial's constant term `f(0)`.
//! - Field operations use fixed iteration counts and avoid secret-dependent
//!   lookup tables and source-level branches. Compiler-level constant-time
//!   behavior is not formally verified.
//!
//! Reconstruction does not authenticate shares. Callers must validate the
//! reconstructed key by opening authenticated ciphertext or using an
//! equivalent trusted authenticator before accepting it.
//!
//! # Constraints
//!
//! - `2 <= threshold <= count <= 10`
//! - Secret is exactly 32 bytes (fits a `SymmetricKey` or any 32-byte key).
//!
//! # TODO
//!
//! Re-evaluate replacement crates during dependency reviews. Adopt one only
//! after its relevant implementation has a published independent audit.

use rand_core::OsRng;
use rand_core::RngCore;
use std::fmt;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Error type for Shamir's Secret Sharing operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShamirError {
    #[error("Threshold must be at least 2, got {0}")]
    ThresholdTooLow(u8),

    #[error("Threshold cannot exceed 10, got {0}")]
    ThresholdTooHigh(u8),

    #[error("Threshold cannot exceed count, got threshold={0}, count={1}")]
    ThresholdExceedsCount(u8, u8),

    #[error("Count must be at least 2, got {0}")]
    CountTooLow(u8),

    #[error("Count cannot exceed 10, got {0}")]
    CountTooHigh(u8),

    #[error("Secret must be exactly 32 bytes, got {0}")]
    InvalidSecretLength(usize),

    #[error("Need at least {required} shares to reconstruct, got {got}")]
    InsufficientShares { required: u8, got: usize },

    #[error("Duplicate share indices detected")]
    DuplicateIndices,

    #[error("Division by zero in GF(256)")]
    DivisionByZero,

    #[error("Reconstruction failed: polynomial evaluation mismatch")]
    ReconstructionFailed,
}

/// A single share in the Shamir scheme.
///
/// `index` is a non-zero byte identifier (the x-coordinate).
/// `value` is the 32-byte y-coordinate `f(index)`.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Share {
    /// Non-zero byte identifier for this share (x-coordinate).
    pub index: u8,
    /// 32-byte share value (y-coordinate).
    pub value: [u8; 32],
}

impl fmt::Debug for Share {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Share")
            .field("index", &self.index)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// Splits a 32-byte secret into `count` shares with a `threshold`.
///
/// Any `threshold` shares can reconstruct the secret; fewer than `threshold`
/// shares reveal no information (information-theoretic security).
///
/// # Arguments
/// * `secret` - The 32-byte secret to split.
/// * `threshold` - Minimum shares needed to reconstruct (t, 2 <= t <= count).
/// * `count` - Total number of shares to generate (n, 2 <= n <= 10).
///
/// # Errors
/// Returns [`ShamirError`] if parameters are invalid.
pub fn split(secret: &[u8; 32], threshold: u8, count: u8) -> Result<Vec<Share>, ShamirError> {
    validate_params(threshold, count)?;

    // For each byte position, build a degree-(threshold-1) polynomial
    // where f(0) = secret[i].
    // Coefficients are random bytes; the constant term is the secret byte.
    let mut shares: Vec<Share> = (1..=count)
        .map(|i| Share {
            index: i,
            value: [0u8; 32],
        })
        .collect();

    for (byte_pos, secret_byte) in secret.iter().enumerate() {
        // Build polynomial coefficients: c[0] = secret, c[1..t-1] = random
        let mut coeffs = vec![0u8; threshold as usize];
        coeffs[0] = *secret_byte;
        OsRng.fill_bytes(&mut coeffs[1..]);

        // Evaluate polynomial at each share index
        for share in &mut shares {
            share.value[byte_pos] = eval_polynomial(&coeffs, share.index);
        }

        coeffs.zeroize();
    }

    Ok(shares)
}

/// Reconstructs the 32-byte secret from at least `threshold` shares.
///
/// Uses Lagrange interpolation at x=0 to recover the secret.
///
/// # Arguments
/// * `shares` - A slice of shares. Must contain at least `threshold` unique
///   shares with distinct indices.
/// * `threshold` - Minimum shares required by the original split.
///
/// # Errors
/// Returns [`ShamirError`] if shares are insufficient or have duplicate indices.
pub fn reconstruct(shares: &[Share], threshold: u8) -> Result<[u8; 32], ShamirError> {
    if threshold < 2 {
        return Err(ShamirError::ThresholdTooLow(threshold));
    }
    if threshold > 10 {
        return Err(ShamirError::ThresholdTooHigh(threshold));
    }
    if shares.len() < threshold as usize {
        return Err(ShamirError::InsufficientShares {
            required: threshold,
            got: shares.len(),
        });
    }

    // Check for duplicate indices
    let mut seen = std::collections::HashSet::new();
    for share in shares {
        if !seen.insert(share.index) {
            return Err(ShamirError::DuplicateIndices);
        }
    }

    let mut secret = [0u8; 32];

    for (byte_pos, secret_byte) in secret.iter_mut().enumerate() {
        // Collect (x_i, y_i) pairs for this byte position
        let mut points: Vec<(u8, u8)> = shares
            .iter()
            .map(|s| (s.index, s.value[byte_pos]))
            .collect();

        let interpolation = lagrange_interpolate_at_zero(&points);
        for (index, value) in &mut points {
            index.zeroize();
            value.zeroize();
        }
        *secret_byte = interpolation?;
    }

    Ok(secret)
}

/// Validates Shamir parameters.
fn validate_params(threshold: u8, count: u8) -> Result<(), ShamirError> {
    if threshold < 2 {
        return Err(ShamirError::ThresholdTooLow(threshold));
    }
    if count < 2 {
        return Err(ShamirError::CountTooLow(count));
    }
    if count > 10 {
        return Err(ShamirError::CountTooHigh(count));
    }
    if threshold > count {
        return Err(ShamirError::ThresholdExceedsCount(threshold, count));
    }
    Ok(())
}

/// Evaluates a polynomial at point `x` in GF(256).
///
/// `coeffs[0]` is the constant term; `coeffs[i]` is the coefficient of x^i.
fn eval_polynomial(coeffs: &[u8], x: u8) -> u8 {
    let mut result = 0u8;
    let mut x_power = 1u8; // x^0

    for coeff in coeffs {
        result ^= gf_mul(*coeff, x_power);
        x_power = gf_mul(x_power, x);
    }

    result
}

/// Lagrange interpolation at x=0 in GF(256).
///
/// Given points (x_i, y_i), computes f(0) where f is the unique degree-(n-1)
/// polynomial passing through all points.
///
/// f(0) = sum_i [ y_i * prod_{j!=i} (x_j / (x_j - x_i)) ]
fn lagrange_interpolate_at_zero(points: &[(u8, u8)]) -> Result<u8, ShamirError> {
    let mut result = 0u8;

    for (i, &(xi, yi)) in points.iter().enumerate() {
        if xi == 0 {
            // This shouldn't happen because share indices are 1..=n, but
            // guard against it for robustness.
            return Err(ShamirError::ReconstructionFailed);
        }

        let mut numerator = 1u8;
        let mut denominator = 1u8;

        for (j, &(xj, _)) in points.iter().enumerate() {
            if i == j {
                continue;
            }
            if xi == xj {
                return Err(ShamirError::DuplicateIndices);
            }

            numerator = gf_mul(numerator, xj);
            denominator = gf_mul(denominator, xi ^ xj); // subtraction = addition in GF(2^8)
        }

        let lagrange_coeff = gf_mul(yi, gf_div(numerator, denominator)?);
        result ^= lagrange_coeff;
    }

    Ok(result)
}

/// GF(256) multiplication: carryless multiply then reduce mod 0x11b.
fn gf_mul(a: u8, b: u8) -> u8 {
    let mut result = 0u8;
    let mut a = a;
    let mut b = b;

    for _ in 0..8 {
        let add_mask = 0u8.wrapping_sub(b & 1);
        result ^= a & add_mask;

        let reduction_mask = 0u8.wrapping_sub(a >> 7);
        a = (a << 1) ^ (0x1b & reduction_mask);
        b >>= 1;
    }

    result
}

/// GF(256) division: a / b = a * b^-1.
fn gf_div(a: u8, b: u8) -> Result<u8, ShamirError> {
    Ok(gf_mul(a, gf_inv(b)?))
}

/// GF(256) multiplicative inverse using a fixed addition chain for a^254.
fn gf_inv(a: u8) -> Result<u8, ShamirError> {
    if a == 0 {
        return Err(ShamirError::DivisionByZero);
    }

    let a2 = gf_mul(a, a);
    let a4 = gf_mul(a2, a2);
    let a8 = gf_mul(a4, a4);
    let a16 = gf_mul(a8, a8);
    let a32 = gf_mul(a16, a16);
    let a64 = gf_mul(a32, a32);
    let a128 = gf_mul(a64, a64);

    let a6 = gf_mul(a2, a4);
    let a14 = gf_mul(a6, a8);
    let a30 = gf_mul(a14, a16);
    let a62 = gf_mul(a30, a32);
    let a126 = gf_mul(a62, a64);
    Ok(gf_mul(a126, a128))
}

// INLINE_TEST_REQUIRED: GF(256) arithmetic and Shamir split/reconstruct logic are
// security-critical; inline tests keep the field operations next to the code they
// verify and make regression tests cheap to add.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn round_trip_2_of_3() {
        let secret = [42u8; 32];
        let shares = split(&secret, 2, 3).unwrap();
        assert_eq!(shares.len(), 3);

        // Reconstruct from any 2 shares
        let reconstructed = reconstruct(&shares[0..2], 2).unwrap();
        assert_eq!(reconstructed, secret);

        let reconstructed = reconstruct(&shares[1..3], 2).unwrap();
        assert_eq!(reconstructed, secret);

        let reconstructed = reconstruct(&[shares[0].clone(), shares[2].clone()], 2).unwrap();
        assert_eq!(reconstructed, secret);
    }

    // @internal
    #[test]
    fn round_trip_3_of_5() {
        let secret = [
            0xABu8, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54,
            0x32, 0x10, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC,
            0xDD, 0xEE, 0xFF, 0x00,
        ];
        let shares = split(&secret, 3, 5).unwrap();
        assert_eq!(shares.len(), 5);

        // Reconstruct from any 3 shares
        let reconstructed = reconstruct(&shares[0..3], 3).unwrap();
        assert_eq!(reconstructed, secret);

        let reconstructed = reconstruct(&shares[2..5], 3).unwrap();
        assert_eq!(reconstructed, secret);
    }

    // @internal
    #[test]
    fn reconstruct_rejects_threshold_minus_one() {
        let secret = [
            1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let shares = split(&secret, 3, 5).unwrap();

        assert_eq!(
            reconstruct(&shares[0..2], 3),
            Err(ShamirError::InsufficientShares {
                required: 3,
                got: 2,
            })
        );
    }

    // @internal
    #[test]
    fn reconstruct_rejects_invalid_thresholds() {
        let secret = [42u8; 32];
        let shares = split(&secret, 2, 3).unwrap();

        assert_eq!(
            reconstruct(&shares, 0),
            Err(ShamirError::ThresholdTooLow(0))
        );
        assert_eq!(
            reconstruct(&shares, 1),
            Err(ShamirError::ThresholdTooLow(1))
        );
        assert_eq!(
            reconstruct(&shares, 11),
            Err(ShamirError::ThresholdTooHigh(11))
        );
    }

    // @internal
    #[test]
    fn duplicate_indices_error() {
        let secret = [42u8; 32];
        let mut shares = split(&secret, 2, 3).unwrap();
        shares[1].index = shares[0].index; // duplicate
        assert_eq!(reconstruct(&shares, 2), Err(ShamirError::DuplicateIndices));
    }

    // @internal
    #[test]
    fn zero_share_index_error() {
        let secret = [42u8; 32];
        let mut shares = split(&secret, 2, 3).unwrap();
        shares[0].index = 0;

        assert_eq!(
            reconstruct(&shares[0..2], 2),
            Err(ShamirError::ReconstructionFailed)
        );
    }

    // @internal
    #[test]
    fn invalid_params_rejected() {
        let secret = [42u8; 32];

        assert_eq!(split(&secret, 1, 3), Err(ShamirError::ThresholdTooLow(1)));
        assert_eq!(
            split(&secret, 4, 3),
            Err(ShamirError::ThresholdExceedsCount(4, 3))
        );
        assert_eq!(split(&secret, 2, 1), Err(ShamirError::CountTooLow(1)));
        assert_eq!(split(&secret, 2, 11), Err(ShamirError::CountTooHigh(11)));
    }

    // @internal
    #[test]
    fn all_zeros_secret() {
        let secret = [0u8; 32];
        let shares = split(&secret, 2, 3).unwrap();
        let reconstructed = reconstruct(&shares, 2).unwrap();
        assert_eq!(reconstructed, secret);
    }

    // @internal
    #[test]
    fn all_ones_secret() {
        let secret = [0xFFu8; 32];
        let shares = split(&secret, 2, 3).unwrap();
        let reconstructed = reconstruct(&shares, 2).unwrap();
        assert_eq!(reconstructed, secret);
    }

    // @internal
    #[test]
    fn deterministic_secret_independence() {
        // Same secret split twice should produce different shares (random coefficients)
        let secret = [0xABu8; 32];
        let shares1 = split(&secret, 2, 3).unwrap();
        let shares2 = split(&secret, 2, 3).unwrap();

        for i in 0..3 {
            assert_ne!(
                shares1[i].value, shares2[i].value,
                "shares should differ across independent splits"
            );
        }
    }

    // @internal
    #[test]
    fn gf_mul_associative() {
        // (a * b) * c == a * (b * c)
        for a in [0u8, 1, 0x53, 0xCA, 0xFF] {
            for b in [0u8, 1, 0x53, 0xCA, 0xFF] {
                for c in [0u8, 1, 0x53, 0xCA, 0xFF] {
                    let left = gf_mul(gf_mul(a, b), c);
                    let right = gf_mul(a, gf_mul(b, c));
                    assert_eq!(left, right, "GF mul not associative for {a},{b},{c}");
                }
            }
        }
    }

    // @internal
    #[test]
    fn gf_mul_commutative() {
        for a in [0u8, 1, 0x53, 0xCA, 0xFF] {
            for b in [0u8, 1, 0x53, 0xCA, 0xFF] {
                assert_eq!(gf_mul(a, b), gf_mul(b, a), "GF mul not commutative");
            }
        }
    }

    // @internal
    #[test]
    fn gf_inv_identity() {
        // a * a^-1 == 1 for all a != 0
        for a in 1..=255u8 {
            let inv = gf_inv(a).unwrap();
            let product = gf_mul(a, inv);
            assert_eq!(product, 1, "GF inverse failed for {a}");
        }
    }

    // @internal
    #[test]
    fn gf_inv_rejects_zero() {
        assert_eq!(gf_inv(0), Err(ShamirError::DivisionByZero));
        assert_eq!(gf_div(1, 0), Err(ShamirError::DivisionByZero));
    }

    // @internal
    #[test]
    fn gf_arithmetic_matches_aes_known_answers() {
        assert_eq!(gf_mul(0x57, 0x13), 0xfe);
        assert_eq!(gf_inv(0x53), Ok(0xca));
        assert_eq!(gf_div(0x57, 0x13), Ok(0x6b));
    }

    // @internal
    #[test]
    fn deterministic_reconstruction_fixture() {
        let shares = [
            Share {
                index: 1,
                value: [0x2d; 32],
            },
            Share {
                index: 2,
                value: [0x24; 32],
            },
        ];

        assert_eq!(reconstruct(&shares, 2), Ok([0x2a; 32]));
    }

    // @internal
    #[test]
    fn gf_mul_identity_and_zero() {
        for a in [0u8, 1, 0x53, 0xCA, 0xFF] {
            assert_eq!(gf_mul(a, 0), 0, "a * 0 != 0");
            assert_eq!(gf_mul(a, 1), a, "a * 1 != a");
        }
    }

    // @internal
    #[test]
    fn wrong_shares_fail() {
        // Create shares for two different secrets; mixing them should not reconstruct
        let secret1 = [0xAAu8; 32];
        let secret2 = [0xBBu8; 32];
        let shares1 = split(&secret1, 2, 3).unwrap();
        let shares2 = split(&secret2, 2, 3).unwrap();

        // Mix one share from each set
        let mixed = vec![shares1[0].clone(), shares2[1].clone()];
        let result = reconstruct(&mixed, 2).unwrap();
        assert_ne!(result, secret1);
        assert_ne!(result, secret2);
    }

    // @internal
    #[test]
    fn edge_case_max_shares() {
        let secret = [
            0xDEu8, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC,
            0xDE, 0xF0, 0x0F, 0x1E, 0x2D, 0x3C, 0x4B, 0x5A, 0x69, 0x78, 0x87, 0x96, 0xA5, 0xB4,
            0xC3, 0xD2, 0xE1, 0xF0,
        ];
        let shares = split(&secret, 2, 10).unwrap();
        assert_eq!(shares.len(), 10);

        let reconstructed = reconstruct(&shares, 2).unwrap();
        assert_eq!(reconstructed, secret);
    }

    // @internal
    #[test]
    fn share_manual_zeroize_clears_fields() {
        let secret = [0xABu8; 32];
        let mut shares = split(&secret, 2, 3).unwrap();
        shares[0].zeroize();
        assert_eq!(shares[0].index, 0);
        assert_eq!(shares[0].value, [0u8; 32]);
    }

    // @internal
    #[test]
    fn share_debug_redacts_value() {
        let share = Share {
            index: 1,
            value: [0xAB; 32],
        };

        let debug = format!("{share:?}");

        assert_eq!(debug, "Share { index: 1, value: \"[REDACTED]\" }");
        assert!(!debug.contains("171"));
    }
}
