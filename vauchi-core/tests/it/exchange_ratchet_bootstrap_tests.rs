// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the shared exchange crypto seam: `exchange::key_order`
//! and `exchange::ratchet_bootstrap` — the role rule and transcript
//! ordering both session types must agree on.

use proptest::prelude::*;
use vauchi_core::crypto::*;
use vauchi_core::exchange::key_order::{is_initiator, sorted_pair};
use vauchi_core::exchange::ratchet_bootstrap::{RatchetBootstrapError, bootstrap_exchange_ratchet};

// @internal
#[test]
fn smaller_identity_key_takes_initiator_role() {
    let smaller = [1u8; 32];
    let larger = [2u8; 32];
    assert!(is_initiator(&smaller, &larger));
    assert!(!is_initiator(&larger, &smaller));
}

// @internal
#[test]
fn equal_identity_keys_yield_responder_on_both_sides() {
    let same = [7u8; 32];
    assert!(!is_initiator(&same, &same));
}

// @internal
#[test]
fn role_decision_uses_full_key_not_prefix() {
    let mut a = [9u8; 32];
    let mut b = [9u8; 32];
    a[31] = 1;
    b[31] = 2;
    assert!(is_initiator(&a, &b));
    assert!(!is_initiator(&b, &a));
}

// @internal
#[test]
fn sorted_pair_orders_lexicographically() {
    let lo = [1u8, 2, 3];
    let hi = [1u8, 2, 4];
    assert_eq!(sorted_pair(&lo[..], &hi[..]), (&lo[..], &hi[..]));
    assert_eq!(sorted_pair(&hi[..], &lo[..]), (&lo[..], &hi[..]));
}

// @internal
#[test]
fn sorted_pair_equal_inputs_keep_argument_order() {
    let a = [5u8; 16];
    let b = [5u8; 16];
    let (first, second) = sorted_pair(&a, &b);
    assert_eq!(first, &a);
    assert_eq!(second, &b);
}

// @internal
#[test]
fn bootstrap_builds_interoperable_role_complementary_ratchets() {
    let shared = SymmetricKey::from_bytes([42u8; 32]);
    let initiator_id = [1u8; 32];
    let responder_id = [2u8; 32];

    let responder_eph = X3DHKeyPair::generate();
    let responder_eph_public = *responder_eph.public_key();

    let (mut initiator_ratchet, initiator_flag) = bootstrap_exchange_ratchet(
        &shared,
        &initiator_id,
        &responder_id,
        Some(responder_eph_public),
        None,
    )
    .unwrap();
    let (mut responder_ratchet, responder_flag) = bootstrap_exchange_ratchet(
        &shared,
        &responder_id,
        &initiator_id,
        None,
        Some(responder_eph),
    )
    .unwrap();

    assert!(initiator_flag);
    assert!(!responder_flag);

    let msg = initiator_ratchet.encrypt(b"first contact").unwrap();
    assert_eq!(
        responder_ratchet.decrypt(&msg).unwrap(),
        b"first contact".to_vec()
    );
    let reply = responder_ratchet.encrypt(b"reply").unwrap();
    assert_eq!(
        initiator_ratchet.decrypt(&reply).unwrap(),
        b"reply".to_vec()
    );
}

// @internal
#[test]
fn initiator_without_peer_ephemeral_is_rejected() {
    let shared = SymmetricKey::from_bytes([42u8; 32]);
    let result = bootstrap_exchange_ratchet(
        &shared,
        &[1u8; 32],
        &[2u8; 32],
        None,
        Some(X3DHKeyPair::generate()),
    );
    assert_eq!(
        result.unwrap_err(),
        RatchetBootstrapError::MissingPeerEphemeral
    );
}

// @internal
#[test]
fn responder_without_own_ephemeral_is_rejected() {
    let shared = SymmetricKey::from_bytes([42u8; 32]);
    let result = bootstrap_exchange_ratchet(&shared, &[2u8; 32], &[1u8; 32], Some([3u8; 32]), None);
    assert_eq!(
        result.unwrap_err(),
        RatchetBootstrapError::MissingOurEphemeral
    );
}

proptest! {
    // @internal
    #[test]
    fn roles_are_complementary_and_ratchets_interoperate(
        id_a in proptest::array::uniform32(any::<u8>()),
        id_b in proptest::array::uniform32(any::<u8>()),
        shared_bytes in proptest::array::uniform32(any::<u8>()),
    ) {
        prop_assume!(id_a != id_b);

        prop_assert_ne!(is_initiator(&id_a, &id_b), is_initiator(&id_b, &id_a));

        let shared = SymmetricKey::from_bytes(shared_bytes);
        let (init_id, resp_id) = if is_initiator(&id_a, &id_b) {
            (id_a, id_b)
        } else {
            (id_b, id_a)
        };
        let responder_eph = X3DHKeyPair::generate();
        let responder_eph_public = *responder_eph.public_key();

        let (mut initiator_ratchet, initiator_flag) = bootstrap_exchange_ratchet(
            &shared, &init_id, &resp_id, Some(responder_eph_public), None,
        ).unwrap();
        let (mut responder_ratchet, responder_flag) = bootstrap_exchange_ratchet(
            &shared, &resp_id, &init_id, None, Some(responder_eph),
        ).unwrap();
        prop_assert!(initiator_flag);
        prop_assert!(!responder_flag);

        let msg = initiator_ratchet.encrypt(b"probe").unwrap();
        prop_assert_eq!(responder_ratchet.decrypt(&msg).unwrap(), b"probe".to_vec());
        let reply = responder_ratchet.encrypt(b"echo").unwrap();
        prop_assert_eq!(initiator_ratchet.decrypt(&reply).unwrap(), b"echo".to_vec());
    }

    // @internal
    #[test]
    fn sorted_pair_is_symmetric(
        a in proptest::collection::vec(any::<u8>(), 0..64),
        b in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let forward = sorted_pair(&a[..], &b[..]);
        let backward = sorted_pair(&b[..], &a[..]);
        prop_assert_eq!(forward.0, backward.0);
        prop_assert_eq!(forward.1, backward.1);
        prop_assert!(forward.0 <= forward.1);
    }
}
