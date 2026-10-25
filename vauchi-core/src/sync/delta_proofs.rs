// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Kani proof for the CEK-wrapped card update a contact sends through the
//! ratchet (#757).
//!
//! Run with `cargo kani -p vauchi-core -Z stubbing --harness sync::delta_proofs`.
//! Bounded to inputs of at most `MAX_PROVEN_INPUT_LEN` bytes, for the host
//! pointer width only. `format!` is stubbed because CBMC stalls on the
//! formatting machinery, and the decoder uses it only for error messages.

use super::delta::CekWrappedPayload;

const HEADER_LEN: usize = 132;
const MAX_PROVEN_INPUT_LEN: usize = HEADER_LEN + 4;

fn format_without_text(_: core::fmt::Arguments<'_>) -> String {
    String::new()
}

fn declared_ciphertext_len(input: &[u8]) -> usize {
    u32::from_be_bytes(input[128..132].try_into().expect("4 bytes")) as usize
}

#[kani::proof]
#[kani::unwind(138)]
#[kani::stub(alloc::fmt::format, format_without_text)]
fn cek_wrapped_payload_decode_never_panics_and_reads_each_field_at_its_offset() {
    let bytes: [u8; MAX_PROVEN_INPUT_LEN] = kani::any();
    let len: usize = kani::any_where(|len| *len <= MAX_PROVEN_INPUT_LEN);
    let input = &bytes[..len];

    match CekWrappedPayload::decode(input) {
        Ok(payload) => {
            assert!(len >= HEADER_LEN);
            assert_eq!(payload.cek[..], input[0..32]);
            assert_eq!(payload.signature[..], input[32..96]);
            assert_eq!(payload.nonce[..], input[96..128]);
            let ct_len = declared_ciphertext_len(input);
            assert_eq!(
                payload.cek_ciphertext[..],
                input[HEADER_LEN..HEADER_LEN + ct_len]
            );
        }
        Err(_) => {
            assert!(len < HEADER_LEN || declared_ciphertext_len(input) > len - HEADER_LEN)
        }
    }
}
