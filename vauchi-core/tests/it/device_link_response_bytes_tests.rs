// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A device-link response whose sync payload is cut short still decodes,
//! with the payload dropped instead of a slice past the end
//! (vauchi/private#522).

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::exchange::DeviceLinkResponse;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};

const SEED: [u8; 32] = [9u8; 32];

fn response(sync_payload: &str) -> DeviceLinkResponse {
    let registry = DeviceRegistry::new(
        DeviceInfo::derive(&SEED, 0, "phone".into(), 0).to_registered(&SEED),
        &SigningKeyPair::from_seed(&SEED),
    );
    DeviceLinkResponse::with_sync_payload(SEED, "Alice".into(), 1, registry, sync_payload.into())
}

// @internal
#[test]
fn a_whole_sync_payload_round_trips() {
    let bytes = response(r#"{"contacts":[]}"#).to_bytes();

    let decoded = DeviceLinkResponse::from_bytes(&bytes).unwrap();

    assert_eq!(decoded.sync_payload_json(), r#"{"contacts":[]}"#);
}

// @internal
#[test]
fn a_sync_payload_cut_short_decodes_as_empty() {
    let bytes = response(r#"{"contacts":[]}"#).to_bytes();

    for cut in [1, 5] {
        let decoded = DeviceLinkResponse::from_bytes(&bytes[..bytes.len() - cut]).unwrap();
        assert_eq!(decoded.sync_payload_json(), "", "cut {cut} bytes");
    }
}

// @internal
#[test]
fn every_prefix_is_refused_or_decoded_without_panicking() {
    let full = response(r#"{"contacts":[]}"#);
    let bytes = full.to_bytes();
    let sync_section = 4 + r#"{"contacts":[]}"#.len();
    let registry_end = bytes.len() - sync_section;

    for n in 0..=bytes.len() {
        let outcome = std::panic::catch_unwind(|| DeviceLinkResponse::from_bytes(&bytes[..n]));
        let decoded =
            outcome.unwrap_or_else(|_| panic!("from_bytes panicked on a {n}-byte prefix"));
        if n < registry_end {
            assert!(
                decoded.is_err(),
                "a {n}-byte prefix ends inside the registry"
            );
        } else if n < bytes.len() {
            assert_eq!(decoded.unwrap().sync_payload_json(), "", "{n}-byte prefix");
        }
    }
}

// @internal
#[test]
fn lengths_claiming_more_than_is_there_are_refused() {
    let bytes = response("").to_bytes();
    let name_len = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    let registry_len_at = 36 + name_len + 4;

    let mut long_name = bytes.clone();
    long_name[32..36].copy_from_slice(&(name_len as u32 + 1000).to_le_bytes());
    let mut long_registry = bytes.clone();
    long_registry[registry_len_at..registry_len_at + 4].copy_from_slice(&100_000u32.to_le_bytes());

    for tampered in [long_name, long_registry] {
        let outcome = std::panic::catch_unwind(|| DeviceLinkResponse::from_bytes(&tampered));
        assert!(matches!(outcome, Ok(Err(_))), "refused, not panicked");
    }
}
