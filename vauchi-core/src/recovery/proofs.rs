// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Kani proof for the recovery claim a stranger can paste into the
//! recovery-help screen (#757).
//!
//! Run with `cargo kani -p vauchi-core --harness recovery::proofs`.
//! Bounded to inputs of at most `MAX_PROVEN_INPUT_LEN` bytes, for the host
//! pointer width only.

use super::RecoveryClaim;
use crate::identifiers::IdentityKey;

const CLAIM_LEN: usize = 73;
const MAX_PROVEN_INPUT_LEN: usize = CLAIM_LEN + 2;

#[kani::proof]
#[kani::unwind(77)]
fn recovery_claim_parse_never_panics_and_reads_each_field_at_its_offset() {
    let bytes: [u8; MAX_PROVEN_INPUT_LEN] = kani::any();
    let len: usize = kani::any_where(|len| *len <= MAX_PROVEN_INPUT_LEN);
    let input = &bytes[..len];

    match RecoveryClaim::from_bytes(input) {
        Ok(claim) => {
            assert!(len >= CLAIM_LEN);
            assert_eq!(input[0], 1);
            let old_pk: [u8; 32] = input[1..33].try_into().expect("32 bytes");
            let new_pk: [u8; 32] = input[33..65].try_into().expect("32 bytes");
            assert_eq!(*claim.old_pk(), IdentityKey::from_bytes(old_pk));
            assert_eq!(*claim.new_pk(), IdentityKey::from_bytes(new_pk));
            let timestamp = u64::from_le_bytes(input[65..73].try_into().expect("8 bytes"));
            assert_eq!(claim.timestamp(), timestamp);
        }
        Err(_) => assert!(len < CLAIM_LEN || input[0] != 1),
    }
}
