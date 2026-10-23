// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Kani proof for the BLE advertisement any nearby device can send (#757).
//!
//! Run with `cargo kani -p vauchi-core --harness ble::proofs`.
//! Bounded to inputs of at most `MAX_PROVEN_INPUT_LEN` bytes, for the host
//! pointer width only.

use super::BLEAdvertisement;

const ADVERTISEMENT_LEN: usize = 128;
const MAX_PROVEN_INPUT_LEN: usize = ADVERTISEMENT_LEN + 2;

#[kani::proof]
#[kani::unwind(132)]
fn ble_advertisement_parse_never_panics_and_reads_each_field_at_its_offset() {
    let bytes: [u8; MAX_PROVEN_INPUT_LEN] = kani::any();
    let len: usize = kani::any_where(|len| *len <= MAX_PROVEN_INPUT_LEN);
    let input = &bytes[..len];

    match BLEAdvertisement::from_bytes(input) {
        Ok(advertisement) => {
            assert!(len >= ADVERTISEMENT_LEN);
            assert_eq!(advertisement.exchange_token, input[0..32]);
            assert_eq!(advertisement.public_key, input[32..64]);
            assert_eq!(advertisement.signature, input[64..128]);
        }
        Err(_) => assert!(len < ADVERTISEMENT_LEN),
    }
}
