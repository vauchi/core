// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based tests for GuardianToken and sealed-box.
//!
//! Traces to: features/contact_recovery.feature
//! - @recovery @trust: guardian token properties
//! - @recovery @relay: sealed-box encryption properties

use proptest::prelude::*;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::guardian::GuardianToken;
use vauchi_core::recovery::sealed_box;
use x25519_dalek::{PublicKey, StaticSecret};

proptest! {
    // @internal
    #[test]
    fn valid_tokens_always_verify(seed1 in any::<[u8; 32]>(), seed2 in any::<[u8; 32]>()) {
        let designator = SigningKeyPair::from_seed(&seed1);
        let guardian = SigningKeyPair::from_seed(&seed2);

        let token = GuardianToken::create(&designator, guardian.public_key(), 0);

        prop_assert!(token.verify());
    }

    // @internal
    #[test]
    fn wrong_signer_never_verifies(
        seed1 in any::<[u8; 32]>(),
        seed2 in any::<[u8; 32]>(),
        seed3 in any::<[u8; 32]>(),
    ) {
        prop_assume!(seed1 != seed2);

        let real_designator = SigningKeyPair::from_seed(&seed1);
        let fake_signer = SigningKeyPair::from_seed(&seed2);
        let guardian = SigningKeyPair::from_seed(&seed3);

        let token = GuardianToken::create_with_claimed_pk(
            &fake_signer,
            real_designator.public_key(),
            guardian.public_key(),
            0,
        );

        prop_assert!(!token.verify());
    }

    // @internal
    #[test]
    fn sealed_box_roundtrip(plaintext in prop::collection::vec(any::<u8>(), 0..512)) {
        let recipient_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let recipient_pk = PublicKey::from(&recipient_secret);

        let sealed = sealed_box::seal(&plaintext, &recipient_pk);
        let opened = sealed_box::open(&sealed, &recipient_secret)
            .expect("open must succeed with the correct recipient key");

        prop_assert_eq!(opened, plaintext);
    }
}
