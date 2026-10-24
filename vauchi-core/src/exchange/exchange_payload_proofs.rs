// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Kani proof for the 174-byte exchange payload that the QR, NFC and BLE
//! formats share (#757).
//!
//! Run with `cargo kani -p vauchi-core --harness exchange_payload_proofs`.
//! Bounded to inputs of at most `MAX_PROVEN_INPUT_LEN` bytes, for the host
//! pointer width only.

use super::ExchangeError;
use super::exchange_payload::{EXCHANGE_PAYLOAD_SIZE, parse_exchange_payload};

/// Two bytes past the payload, so trailing bytes are covered too.
const MAX_PROVEN_INPUT_LEN: usize = EXCHANGE_PAYLOAD_SIZE + 2;

#[kani::proof]
#[kani::unwind(178)]
fn parse_exchange_payload_never_panics_and_reads_each_field_at_its_offset() {
    let bytes: [u8; MAX_PROVEN_INPUT_LEN] = kani::any();
    let len: usize = kani::any_where(|len| *len <= MAX_PROVEN_INPUT_LEN);
    let input = &bytes[..len];
    let magic: [u8; 4] = kani::any();

    match parse_exchange_payload(input, &magic, ExchangeError::InvalidQRFormat) {
        Ok(parsed) => {
            assert!(len >= EXCHANGE_PAYLOAD_SIZE);
            assert_eq!(input[0..4], magic);
            assert_eq!(parsed.version, 1);
            assert_eq!(input[4], 1);
            assert_eq!(parsed.flags, input[5]);
            assert_eq!(parsed.identity_key, input[6..38]);
            assert_eq!(parsed.exchange_key, input[38..70]);
            assert_eq!(parsed.token, input[70..102]);
            let timestamp = u64::from_be_bytes(input[102..110].try_into().expect("8 bytes"));
            assert_eq!(parsed.timestamp, timestamp);
            assert_eq!(parsed.signature, input[110..174]);
        }
        Err(_) => {
            assert!(len < EXCHANGE_PAYLOAD_SIZE || input[0..4] != magic || input[4] != 1);
        }
    }
}
