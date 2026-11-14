// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};
use proptest::prelude::*;

use super::{COMPRESSED_CONTROL_VERSION, MAX_EXPANDED_ENVELOPE, pack, unpack};
use crate::exchange::genesis::GenesisError;

fn canonical(length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    bytes[0] = 0x05;
    bytes
}

fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn wrapper(bytes: &[u8], stream: &[u8]) -> Vec<u8> {
    let mut wire = vec![COMPRESSED_CONTROL_VERSION];
    wire.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_be_bytes());
    wire.extend_from_slice(&u32::try_from(stream.len()).unwrap().to_be_bytes());
    wire.extend_from_slice(stream);
    assert!(wire.len() <= 4092);
    wire.resize(4092, 0);
    wire
}

// @internal
#[test]
fn small_v1_envelopes_stay_byte_for_byte_identical_through_the_wire_boundary() {
    for length in [1, 1024, 2048, 4091, 4092] {
        let bytes = canonical(length);
        let wire = pack(bytes.clone(), false).unwrap();
        assert_eq!(wire, bytes);
        assert_eq!(unpack(&wire).unwrap().as_ref(), bytes.as_slice());
    }
}

// @internal
#[test]
fn oversized_control_envelopes_always_occupy_the_full_bucket() {
    for length in [4093, 6000, 10_433, 16_571] {
        let bytes = canonical(length);
        let wire = pack(bytes.clone(), true).unwrap();
        assert_eq!(wire.len(), 4092);
        assert_eq!(wire[0], COMPRESSED_CONTROL_VERSION);
        assert_eq!(unpack(&wire).unwrap().as_ref(), bytes.as_slice());
    }
}

// @internal
#[test]
fn private_content_is_not_admitted_to_the_oversized_compression_path() {
    let error = pack(canonical(4093), false).unwrap_err();
    assert!(matches!(error, GenesisError::TooLarge), "{error:?}");
}

// @internal
#[test]
fn expansion_ceiling_and_one_byte_more_are_distinct() {
    let bytes = canonical(16_571);
    let wire = pack(bytes.clone(), true).unwrap();
    assert_eq!(unpack(&wire).unwrap().len(), 16_571);
    let error = pack(canonical(16_572), true).unwrap_err();
    assert!(matches!(error, GenesisError::TooLarge), "{error:?}");
}

// @internal
#[test]
fn an_expansion_bomb_cannot_use_a_smaller_advertised_length() {
    let bytes = canonical(MAX_EXPANDED_ENVELOPE * 2);
    let mut wire = wrapper(&bytes, &compressed(&bytes));
    wire[1..5].copy_from_slice(&16_571u32.to_be_bytes());
    let error = unpack(&wire).unwrap_err();
    assert!(matches!(error, GenesisError::TooLarge), "{error:?}");
}

// @internal
#[test]
fn advertised_lengths_and_zero_padding_are_checked() {
    let bytes = canonical(5000);
    let original = wrapper(&bytes, &compressed(&bytes));
    let mut wrong_expansion = original.clone();
    wrong_expansion[1..5].copy_from_slice(&5001u32.to_be_bytes());
    let mut wrong_stream = original.clone();
    wrong_stream[5..9].copy_from_slice(&u32::MAX.to_be_bytes());
    let mut wrong_padding = original.clone();
    *wrong_padding.last_mut().unwrap() = 1;
    for wire in [
        wrong_expansion,
        wrong_stream,
        wrong_padding,
        original[..4091].to_vec(),
    ] {
        let error = unpack(&wire).unwrap_err();
        assert!(matches!(error, GenesisError::InvalidFormat), "{error:?}");
    }
}

// @internal
#[test]
fn incomplete_corrupt_and_dictionary_streams_fail_closed() {
    let bytes = canonical(5000);
    let stream = compressed(&bytes);
    let mut corrupt = stream.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    let dictionary = vec![0x78, 0xbb, 0, 0, 0, 1, 0];
    for candidate in [
        stream[..stream.len() - 1].to_vec(),
        corrupt,
        dictionary,
        Vec::new(),
    ] {
        let error = unpack(&wrapper(&bytes, &candidate)).unwrap_err();
        assert!(matches!(error, GenesisError::InvalidFormat), "{error:?}");
    }
}

// @internal
#[test]
fn trailing_bytes_and_concatenated_streams_are_not_accepted() {
    let bytes = canonical(5000);
    for suffix in [vec![0], compressed(&bytes)] {
        let mut stream = compressed(&bytes);
        stream.extend_from_slice(&suffix);
        let error = unpack(&wrapper(&bytes, &stream)).unwrap_err();
        assert!(matches!(error, GenesisError::InvalidFormat), "{error:?}");
    }
}

// @internal
#[test]
fn nesting_non_v1_data_and_compressed_small_forms_are_rejected() {
    let mut nested = canonical(5000);
    nested[0] = COMPRESSED_CONTROL_VERSION;
    for bytes in [nested, vec![0x99; 5000], canonical(4092)] {
        let error = unpack(&wrapper(&bytes, &compressed(&bytes))).unwrap_err();
        assert!(matches!(error, GenesisError::InvalidFormat), "{error:?}");
    }
}

proptest! {
    // @internal
    #[test]
    fn bounded_control_roundtrip_preserves_every_byte_or_refuses_overflow(
        tail in prop::collection::vec(any::<u8>(), 0..16_571)
    ) {
        let mut bytes = vec![0x05];
        bytes.extend_from_slice(&tail);
        match pack(bytes.clone(), true) {
            Ok(wire) => {
                prop_assert!(wire.len() <= 4092);
                let decoded = unpack(&wire).unwrap();
                prop_assert_eq!(decoded.as_ref(), bytes.as_slice());
                if bytes.len() <= 4092 {
                    prop_assert_eq!(wire, bytes);
                } else {
                    prop_assert_eq!(wire.len(), 4092);
                }
            }
            Err(GenesisError::TooLarge) => prop_assert!(bytes.len() > 4092),
            Err(error) => prop_assert!(false, "unexpected wire error: {error:?}"),
        }
    }
}
