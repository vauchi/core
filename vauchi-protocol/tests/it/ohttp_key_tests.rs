// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Signed, windowed OHTTP gateway key records (#288): the wire codec and the
//! exact bytes each signature covers.

use proptest::prelude::*;
use vauchi_protocol::ohttp_key::{
    IntermediateCert, MAX_KEY_CONFIG_BYTES, MAX_RECORD_BYTES, RECORD_VERSION, SignedKeyConfig,
    SignedKeyConfigError, WINDOW_SECONDS, key_id_for_window, window_of,
};

fn sample() -> SignedKeyConfig {
    SignedKeyConfig {
        window: 20_366,
        key_config: vec![0x4e, 0x00, 0x20, 0xaa, 0xbb],
        signature: [0x11; 64],
        intermediate: IntermediateCert {
            public_key: [0x22; 32],
            not_before: 1_759_000_000,
            not_after: 1_766_776_000,
            anchor_signature: [0x33; 64],
        },
    }
}

// @internal
#[test]
fn a_record_round_trips() {
    let record = sample();

    assert_eq!(SignedKeyConfig::decode(&record.encode()), Ok(record));
}

// @internal
#[test]
fn the_encoding_starts_with_the_version_and_the_big_endian_window() {
    let encoded = sample().encode();

    assert_eq!(encoded[0], RECORD_VERSION);
    assert_eq!(&encoded[1..9], &20_366u64.to_be_bytes());
}

// @internal
#[test]
fn every_truncation_is_rejected() {
    let encoded = sample().encode();

    for len in 0..encoded.len() {
        assert_eq!(
            SignedKeyConfig::decode(&encoded[..len]),
            Err(SignedKeyConfigError::Truncated),
            "prefix of {len} bytes"
        );
    }
}

// @internal
#[test]
fn trailing_bytes_are_rejected() {
    let mut encoded = sample().encode();
    encoded.push(0);

    assert_eq!(
        SignedKeyConfig::decode(&encoded),
        Err(SignedKeyConfigError::TrailingBytes)
    );
}

/// DC-01: the size bound is checked before anything is parsed.
// @internal
#[test]
fn input_over_the_record_bound_is_rejected_before_parsing() {
    let oversized = vec![RECORD_VERSION; MAX_RECORD_BYTES + 1];

    assert_eq!(
        SignedKeyConfig::decode(&oversized),
        Err(SignedKeyConfigError::TooLarge(MAX_RECORD_BYTES + 1))
    );
}

// @internal
#[test]
fn an_unknown_version_is_rejected() {
    let mut encoded = sample().encode();
    encoded[0] = RECORD_VERSION + 1;

    assert_eq!(
        SignedKeyConfig::decode(&encoded),
        Err(SignedKeyConfigError::UnknownVersion(RECORD_VERSION + 1))
    );
}

// @internal
#[test]
fn an_empty_key_config_is_rejected() {
    let mut record = sample();
    record.key_config.clear();

    assert_eq!(
        SignedKeyConfig::decode(&record.encode()),
        Err(SignedKeyConfigError::EmptyKeyConfig)
    );
}

// @internal
#[test]
fn a_declared_key_config_over_the_bound_is_rejected() {
    let mut encoded = sample().encode();
    let declared = (MAX_KEY_CONFIG_BYTES + 1) as u16;
    encoded[9..11].copy_from_slice(&declared.to_be_bytes());

    assert_eq!(
        SignedKeyConfig::decode(&encoded),
        Err(SignedKeyConfigError::KeyConfigTooLarge(
            MAX_KEY_CONFIG_BYTES + 1
        ))
    );
}

/// The gateway signs these exact bytes; any change is a protocol break.
// @internal
#[test]
fn the_key_config_signing_message_is_domain_window_and_key_config() {
    let message = SignedKeyConfig::signing_message(258, &[0xde, 0xad]);

    let mut expected = b"vauchi-ohttp-keyconfig-v1".to_vec();
    expected.extend_from_slice(&258u64.to_be_bytes());
    expected.extend_from_slice(&[0xde, 0xad]);
    assert_eq!(message, expected);
}

// @internal
#[test]
fn the_intermediate_signing_message_is_domain_key_and_validity() {
    let message = IntermediateCert::signing_message(&[0x07; 32], 10, 20);

    let mut expected = b"vauchi-ohttp-intermediate-v1".to_vec();
    expected.extend_from_slice(&[0x07; 32]);
    expected.extend_from_slice(&10u64.to_be_bytes());
    expected.extend_from_slice(&20u64.to_be_bytes());
    assert_eq!(message, expected);
}

/// Domain separation: the two signatures can never be confused, even over
/// identical payload bytes.
// @internal
#[test]
fn the_two_signing_messages_never_coincide() {
    let key_config = [0x07; 48];

    assert_ne!(
        SignedKeyConfig::signing_message(10, &key_config),
        IntermediateCert::signing_message(&[0x07; 32], 10, 20),
    );
}

// @internal
#[test]
fn windows_are_whole_utc_days_and_key_ids_wrap_at_256() {
    assert_eq!(WINDOW_SECONDS, 86_400);
    assert_eq!(window_of(0), 0);
    assert_eq!(window_of(86_399), 0);
    assert_eq!(window_of(86_400), 1);
    assert_eq!(key_id_for_window(255), 255);
    assert_eq!(key_id_for_window(256), 0);
    assert_eq!(key_id_for_window(20_366), (20_366 % 256) as u8);
}

proptest! {
    // @internal
    #[test]
    fn any_record_round_trips(
        window in any::<u64>(),
        key_config in proptest::collection::vec(any::<u8>(), 1..=MAX_KEY_CONFIG_BYTES),
        signature in any::<[u8; 32]>(),
        public_key in any::<[u8; 32]>(),
        not_before in any::<u64>(),
        not_after in any::<u64>(),
    ) {
        let mut full_signature = [0u8; 64];
        full_signature[..32].copy_from_slice(&signature);
        full_signature[32..].copy_from_slice(&signature);
        let record = SignedKeyConfig {
            window,
            key_config,
            signature: full_signature,
            intermediate: IntermediateCert {
                public_key,
                not_before,
                not_after,
                anchor_signature: full_signature,
            },
        };

        prop_assert_eq!(SignedKeyConfig::decode(&record.encode()), Ok(record));
    }

    // @internal
    #[test]
    fn decoding_is_canonical(bytes in proptest::collection::vec(any::<u8>(), 0..MAX_RECORD_BYTES + 8)) {
        // Any input that decodes re-encodes to exactly itself, so one record
        // has one encoding and a signature cannot be replayed over a variant.
        let canonical = SignedKeyConfig::decode(&bytes)
            .map(|record| record.encode() == bytes)
            .unwrap_or(true);

        prop_assert!(canonical);
    }
}
