// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A dev build can trust a test publisher key given at build time, so a
//! device can take a manifest signed on a workstation from a local relay
//! (vauchi/private#733 T4.2). Release builds trust the production key only.

use proptest::prelude::*;
use vauchi_app::content::{ContentConfig, publisher_key_from_hex};
use vauchi_core::crypto::signing::SigningKeyPair;

// @internal
#[test]
fn a_key_reads_from_its_hex() {
    let key = SigningKeyPair::generate().public_key();
    let parsed = publisher_key_from_hex(&hex::encode(key.as_bytes())).unwrap();
    assert_eq!(parsed.as_bytes(), key.as_bytes());
}

// @internal
#[test]
fn anything_but_32_bytes_of_hex_is_refused() {
    let key = hex::encode([7u8; 32]);
    for bad in [
        String::new(),
        hex::encode([7u8; 31]),
        hex::encode([7u8; 33]),
        format!("{}zz", &key[..62]),
        format!("0x{key}"),
        format!("{key}\0"),
    ] {
        assert!(publisher_key_from_hex(&bad).is_none(), "{bad:?}");
    }
}

// @internal
#[cfg(not(feature = "dev-content-publisher-key"))]
#[test]
fn a_release_build_trusts_only_the_production_key() {
    let production = "9512ff3a3025f0f11ea1c3d9fceabb0d59993e2bbd87fc13b8e42459d8c8b82d";
    assert_eq!(
        hex::encode(ContentConfig::default().publisher_public_key.as_bytes()),
        production
    );
}

proptest! {
    // @internal
    #[test]
    fn every_key_survives_its_hex(bytes in any::<[u8; 32]>()) {
        let parsed = publisher_key_from_hex(&hex::encode(bytes)).unwrap();
        prop_assert_eq!(parsed.as_bytes(), &bytes);
    }
}
