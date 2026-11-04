// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! adr-constraints "Identity signing key uses raw seed directly": the
//! master seed is the Ed25519 secret itself, never an HKDF output, on
//! every device of the identity (private#972, SC-64).

use proptest::prelude::*;
use vauchi_core::identity::Identity;

fn ed25519_public_key_of(secret: &[u8; 32]) -> [u8; 32] {
    ed25519_dalek::SigningKey::from_bytes(secret)
        .verifying_key()
        .to_bytes()
}

// @internal
#[test]
fn a_new_identity_signs_with_its_master_seed_as_the_ed25519_secret() {
    let identity = Identity::create("Alice", 0);

    assert_eq!(
        identity.signing_public_key(),
        &ed25519_public_key_of(identity.master_seed())
    );
}

// @internal
#[test]
fn the_signing_key_is_not_the_derived_exchange_key() {
    let identity = Identity::create("Alice", 0);

    assert_ne!(
        identity.signing_public_key().as_slice(),
        identity.exchange_public_key()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // @internal
    #[test]
    fn a_linked_device_signs_with_the_raw_seed_whatever_its_index(
        seed in any::<[u8; 32]>(),
        device_index in 0u32..64,
    ) {
        let identity =
            Identity::from_device_link(seed, "Alice".into(), device_index, "Laptop".into(), 0);

        prop_assert_eq!(identity.signing_public_key(), &ed25519_public_key_of(&seed));
    }
}
