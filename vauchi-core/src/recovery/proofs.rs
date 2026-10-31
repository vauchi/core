// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Kani proofs for what a stranger can hand the recovery code: the claim a
//! helper scans (#757), a voucher and a guardian token a recovering device
//! adds, and the time bounds they are checked against (private#937).
//!
//! Run with `cargo kani -p vauchi-core --harness recovery::proofs`. Each
//! parser is bounded to inputs a little longer than its exact shape, for
//! the host pointer width only, and is proved to never panic and to accept
//! only that shape, reading each field at its offset (private#930).
//!
//! The claim and time-bound proofs run in `test:formal` (5 s and 3 s). The
//! voucher and token proofs run by hand: the token proof verified in
//! 54 minutes on a 24-thread laptop and the voucher proof had not finished
//! after 40, past the CI job caps (2026-10-31, private#937).
//!
//! `RecoveryProof::from_bytes` has no harness: its postcard body nests a
//! vector of vouchers, and a bound small enough for CBMC proves only that
//! short inputs are refused, which `recovery_parsing_bounds_tests` already
//! shows. Its version and trailing-byte checks are unit-tested there.

use super::guardian::GuardianToken;
use super::{RecoveryClaim, RecoveryProof, RecoveryVoucher};
use crate::identifiers::IdentityKey;

const CLAIM_LEN: usize = 73;
const MAX_PROVEN_CLAIM_INPUT_LEN: usize = CLAIM_LEN + 2;

/// Format, old key, new key, helper key, timestamp, signature.
const CONTACT_VOUCHER_LEN: usize = 1 + 32 + 32 + 32 + 8 + 64;
const MAX_PROVEN_VOUCHER_INPUT_LEN: usize = CONTACT_VOUCHER_LEN + 2;

/// Designator key, guardian key, signature; the created-at varint between
/// them takes one to ten bytes.
const TOKEN_FIXED_LEN: usize = 32 + 32 + 64;
const MAX_VARINT_LEN: usize = 10;
const MAX_PROVEN_TOKEN_INPUT_LEN: usize = TOKEN_FIXED_LEN + MAX_VARINT_LEN + 2;

fn key_at(input: &[u8], offset: usize) -> IdentityKey {
    let bytes: [u8; 32] = input[offset..offset + 32].try_into().expect("32 bytes");
    IdentityKey::from_bytes(bytes)
}

fn u64_at(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(input[offset..offset + 8].try_into().expect("8 bytes"))
}

#[kani::proof]
#[kani::unwind(77)]
fn recovery_claim_parse_never_panics_and_reads_each_field_at_its_offset() {
    let bytes: [u8; MAX_PROVEN_CLAIM_INPUT_LEN] = kani::any();
    let len: usize = kani::any_where(|len| *len <= MAX_PROVEN_CLAIM_INPUT_LEN);
    let input = &bytes[..len];

    match RecoveryClaim::from_bytes(input) {
        Ok(claim) => {
            assert_eq!(len, CLAIM_LEN);
            assert_eq!(input[0], 1);
            assert_eq!(*claim.old_pk(), key_at(input, 1));
            assert_eq!(*claim.new_pk(), key_at(input, 33));
            assert_eq!(claim.timestamp(), u64_at(input, 65));
        }
        Err(_) => assert!(len != CLAIM_LEN || input[0] != 1),
    }
}

#[kani::proof]
#[kani::unwind(175)]
fn recovery_voucher_parse_accepts_exactly_a_contact_voucher() {
    let bytes: [u8; MAX_PROVEN_VOUCHER_INPUT_LEN] = kani::any();
    let len: usize = kani::any_where(|len| *len <= MAX_PROVEN_VOUCHER_INPUT_LEN);
    let input = &bytes[..len];

    match RecoveryVoucher::from_bytes(input) {
        Ok(voucher) => {
            assert_eq!(len, CONTACT_VOUCHER_LEN);
            assert_eq!(input[0], super::CONTACT_VOUCHER_FORMAT);
            assert_eq!(*voucher.old_pk(), key_at(input, 1));
            assert_eq!(*voucher.new_pk(), key_at(input, 33));
            assert_eq!(*voucher.voucher_pk(), key_at(input, 65));
            assert_eq!(voucher.timestamp(), u64_at(input, 97));
            assert!(voucher.guardian_token().is_none());
        }
        // A guardian voucher is at least the header, four length bytes and
        // a token, longer than any input proven here.
        Err(_) => assert!(len != CONTACT_VOUCHER_LEN || input[0] != super::CONTACT_VOUCHER_FORMAT),
    }
}

/// The length of the LEB128 varint at `bytes`: the bytes through the first
/// one without its continuation bit, or `None` past ten bytes.
fn varint_len(bytes: &[u8]) -> Option<usize> {
    bytes
        .iter()
        .take(MAX_VARINT_LEN)
        .position(|byte| byte & 0x80 == 0)
        .map(|position| position + 1)
}

fn varint_value(bytes: &[u8]) -> u64 {
    bytes.iter().enumerate().fold(0u64, |value, (i, byte)| {
        value | (u64::from(byte & 0x7f) << (7 * i))
    })
}

#[kani::proof]
#[kani::unwind(145)]
fn guardian_token_parse_accepts_exactly_one_token() {
    let bytes: [u8; MAX_PROVEN_TOKEN_INPUT_LEN] = kani::any();
    let len: usize = kani::any_where(|len| *len <= MAX_PROVEN_TOKEN_INPUT_LEN);
    let input = &bytes[..len];

    match GuardianToken::from_bytes(input) {
        Ok(token) => {
            let varint = varint_len(&input[64..]).expect("a parsed token has a timestamp");
            assert_eq!(len, TOKEN_FIXED_LEN + varint);
            assert_eq!(*token.designator_pk(), input[..32]);
            assert_eq!(*token.guardian_pk(), input[32..64]);
            assert_eq!(token.created_at(), varint_value(&input[64..64 + varint]));
            assert_eq!(token.signature_bytes()[..], input[64 + varint..]);
        }
        Err(_) => {
            let exact_shape = len > 64
                && varint_len(&input[64..]).is_some_and(|varint| len == TOKEN_FIXED_LEN + varint);
            // Only a ten-byte varint whose last byte carries more than
            // u64 holds is refused at the exact shape.
            assert!(!exact_shape || (len == TOKEN_FIXED_LEN + MAX_VARINT_LEN && input[73] > 1));
        }
    }
}

#[kani::proof]
fn recovery_time_bounds_are_exact_and_never_overflow() {
    let stamped: u64 = kani::any();
    let now: u64 = kani::any();
    let claim = RecoveryClaim::new_with_timestamp([1u8; 32], [2u8; 32], stamped);

    assert_eq!(
        claim.is_expired(now),
        now > stamped && now - stamped > RecoveryClaim::MAX_AGE_SECS
    );
    assert_eq!(
        RecoveryClaim::is_from_the_future(stamped, now),
        stamped > now && stamped - now > RecoveryClaim::MAX_FUTURE_SKEW_SECS
    );

    let proof = RecoveryProof::new([1u8; 32], [2u8; 32], 3, stamped);
    assert!(!proof.is_expired(stamped));
    assert_eq!(
        proof.is_expired(now),
        now > stamped.saturating_add(RecoveryProof::EXPIRY_SECS)
    );
}
