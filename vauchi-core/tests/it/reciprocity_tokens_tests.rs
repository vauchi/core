// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the transport-agnostic reciprocity confirmation-token primitive
//! (`exchange::reciprocity_tokens`). QR, BLE, and multi-stage all derive tokens
//! through this function, so its cross-match property is what lets one shared
//! `ReciprocityConfirmer` verify a peer's token over any channel.

use proptest::prelude::*;
use vauchi_core::exchange::reciprocity_tokens::derive_confirmation_tokens;

// The core invariant: from the SAME shared secret with role-swapped identity
// keys, A's own token equals what B expects of A (and vice versa).
// @internal
#[test]
fn tokens_cross_match_across_peers() {
    let secret = [7u8; 32];
    let alice = [1u8; 32];
    let bob = [2u8; 32];

    let (a_our, a_their) = derive_confirmation_tokens(&secret, &alice, &bob);
    let (b_our, b_their) = derive_confirmation_tokens(&secret, &bob, &alice);

    assert_eq!(
        *a_our, *b_their,
        "A's own token must equal what B expects of A"
    );
    assert_eq!(
        *b_our, *a_their,
        "B's own token must equal what A expects of B"
    );
}

// @internal
#[test]
fn tokens_are_self_asymmetric_and_secret_bound() {
    let secret = [7u8; 32];
    let (a_our, a_their) = derive_confirmation_tokens(&secret, &[1u8; 32], &[2u8; 32]);
    assert_ne!(
        *a_our, *a_their,
        "own vs expected token must differ (echo protection)"
    );

    let (a_our_other_secret, _) = derive_confirmation_tokens(&[8u8; 32], &[1u8; 32], &[2u8; 32]);
    assert_ne!(
        *a_our, *a_our_other_secret,
        "tokens must depend on the shared secret"
    );
}

proptest! {
    // CC-04: the cross-match property holds for arbitrary secrets + ids, and a
    // secret change always changes the token.
    // @internal
    #[test]
    fn cross_match_and_secret_binding_hold(
        secret in any::<[u8; 32]>(),
        other_secret in any::<[u8; 32]>(),
        a in any::<[u8; 32]>(),
        b in any::<[u8; 32]>(),
    ) {
        prop_assume!(a != b);
        prop_assume!(secret != other_secret);

        let (a_our, _) = derive_confirmation_tokens(&secret, &a, &b);
        let (_, b_their) = derive_confirmation_tokens(&secret, &b, &a);
        prop_assert_eq!(*a_our, *b_their, "cross-match must hold for any inputs");

        let (a_our_2, _) = derive_confirmation_tokens(&other_secret, &a, &b);
        prop_assert_ne!(*a_our, *a_our_2, "a different secret must change the token");
    }
}
