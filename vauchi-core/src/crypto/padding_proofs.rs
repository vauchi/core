// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Kani proofs for the padding wire format.
//!
//! Run with `cargo kani -p vauchi-core --harness padding_proofs`.
//! Proofs are bounded (inputs up to `MAX_PROVEN_INPUT_LEN` bytes) and hold
//! for the host pointer width only: Kani models 64-bit `usize`, so they say
//! nothing about 32-bit targets such as armeabi-v7a.

use super::padding::unpad;

const LENGTH_PREFIX_SIZE: usize = 4;
const MAX_PROVEN_INPUT_LEN: usize = 12;

#[kani::proof]
#[kani::unwind(14)]
fn unpad_never_panics_and_only_accepts_a_prefixed_plaintext() {
    let bytes: [u8; MAX_PROVEN_INPUT_LEN] = kani::any();
    let input_len: usize = kani::any_where(|len| *len <= MAX_PROVEN_INPUT_LEN);
    let input = &bytes[..input_len];

    match unpad(input) {
        Some(plaintext) => {
            let encoded_len = u32::try_from(plaintext.len()).expect("plaintext fits u32");
            assert_eq!(input[..LENGTH_PREFIX_SIZE], encoded_len.to_be_bytes());
            assert!(input[LENGTH_PREFIX_SIZE..].starts_with(&plaintext));
        }
        None => {
            let prefix_missing = input.len() < LENGTH_PREFIX_SIZE;
            let declared_len_overruns = !prefix_missing && {
                let declared = u32::from_be_bytes([input[0], input[1], input[2], input[3]]);
                declared as usize > input.len() - LENGTH_PREFIX_SIZE
            };
            assert!(prefix_missing || declared_len_overruns);
        }
    }
}
