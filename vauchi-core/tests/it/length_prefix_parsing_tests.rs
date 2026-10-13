// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Length-prefixed parsers refuse lying lengths without panicking
//! (vauchi/private#286 P2.7). Offsets were computed as `4 + len + 32`
//! from a `u32` length: harmless on a 64-bit host, but on a 32-bit target
//! (armv7 Android, wasm32) the sum wraps into an in-bounds slice range or
//! panics. The parsers now read through a checked cursor.

use proptest::prelude::*;
use vauchi_core::exchange::{DeviceLinkRequest, ExchangeError};
use vauchi_core::identity::Identity;

fn with_name_len(mut bytes: Vec<u8>, len: u32) -> Vec<u8> {
    bytes[..4].copy_from_slice(&len.to_le_bytes());
    bytes
}

// @internal
#[test]
fn a_link_request_with_a_maximal_name_length_is_refused() {
    let bytes = DeviceLinkRequest::new("Phone".into(), 1_700_000_000).to_bytes();
    for len in [u32::MAX, u32::MAX - 3, u32::MAX - 35] {
        assert!(matches!(
            DeviceLinkRequest::from_bytes(&with_name_len(bytes.clone(), len)),
            Err(ExchangeError::InvalidQRFormat)
        ));
    }
}

// @internal
#[test]
fn identity_bytes_with_a_maximal_name_length_are_refused() {
    let bytes = Identity::create("Alice", 0).to_storage_bytes();
    for len in [u32::MAX, u32::MAX - 3, u32::MAX - 35] {
        assert!(Identity::from_storage_bytes(&with_name_len(bytes.clone(), len), 0).is_err());
    }
}

proptest! {
    // @internal
    #[test]
    fn a_link_request_parser_never_panics(data in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _ = DeviceLinkRequest::from_bytes(&data);
    }

    // @internal
    #[test]
    fn an_identity_parser_never_panics(data in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _ = Identity::from_storage_bytes(&data, 0);
    }

    // @internal
    #[test]
    fn a_link_request_round_trips(name in "[a-zA-Z0-9 ]{0,40}", ts in any::<u64>()) {
        let request = DeviceLinkRequest::new(name.clone(), ts);
        let parsed = DeviceLinkRequest::from_bytes(&request.to_bytes()).unwrap();
        prop_assert_eq!(parsed.device_name, name);
    }
}
