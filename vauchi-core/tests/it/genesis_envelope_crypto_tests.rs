// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Genesis envelope crypto contract (ADR-068, MR B).
//!
//! A device holding only a contact's `shared_key` (no established session)
//! seals a safety alert into a wire-ordinary `RatchetMessage` rooted in
//! `shared_key`. The receiver opens it statelessly from `shared_key` + the
//! message header. See `planning/todo/2026-07-21-genesis-envelope-plan.md`
//! §REVISION (findings F1, F7, F8, F10).

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::genesis::{GENESIS_MAX_CHAIN_INDEX, GenesisEnvelope, GenesisError};
use vauchi_core::identity::{Identity, RegistryBroadcast};
use vauchi_core::sync::delta::VersionedPayload;
use vauchi_core::sync::safety_alert::{AlertKind, SafetyAlertPayload};

const EPOCH: u64 = 20_100;

fn sender_broadcast(identity: &Identity) -> RegistryBroadcast {
    RegistryBroadcast::new(
        &identity.initial_device_registry(),
        identity.signing_keypair(),
        0,
    )
}

fn signed_alert_bytes(sender: &Identity, recipient: &Identity) -> Vec<u8> {
    let alert = SafetyAlertPayload::new(
        AlertKind::Emergency,
        "check on me".to_string(),
        7_000,
        None,
        [9u8; 32],
        sender,
        recipient.signing_public_key(),
    )
    .expect("alert construction");
    VersionedPayload::encode_alert(&alert)
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn genesis_header_is_fresh_and_never_static_device_exchange_key() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let broadcast = sender_broadcast(&alice);
    let alert = signed_alert_bytes(&alice, &bob);

    let (msg1, _) = GenesisEnvelope::seal(
        &shared,
        &alice,
        bob.signing_public_key(),
        &broadcast,
        EPOCH,
        &alert,
    )
    .expect("seal 1");
    let (msg2, _) = GenesisEnvelope::seal(
        &shared,
        &alice,
        bob.signing_public_key(),
        &broadcast,
        EPOCH,
        &alert,
    )
    .expect("seal 2");

    assert_ne!(
        msg1.dh_public, msg2.dh_public,
        "each genesis message must carry a fresh ephemeral header key (F1)"
    );
    let static_exchange = *alice.device_info().exchange_public_key();
    assert_ne!(
        msg1.dh_public, static_exchange,
        "the header key must never be the static device exchange key (ADR req 1)"
    );
    assert_eq!(msg1.dh_generation, 0, "genesis msg#1 is generation 0");
    assert_eq!(msg1.message_index, 0, "genesis msg#1 is index 0");
    assert_eq!(msg1.previous_chain_length, 0);
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn genesis_roundtrip_recovers_alert_registry_and_device() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let broadcast = sender_broadcast(&alice);
    let alert = signed_alert_bytes(&alice, &bob);

    let (msg, _sender_session) = GenesisEnvelope::seal(
        &shared,
        &alice,
        bob.signing_public_key(),
        &broadcast,
        EPOCH,
        &alert,
    )
    .expect("seal");

    let opened = GenesisEnvelope::open(
        &shared,
        alice.signing_public_key(),
        bob.signing_public_key(),
        &msg,
    )
    .expect("open");

    assert_eq!(opened.epoch, EPOCH);
    assert_eq!(&opened.sender_device_id, alice.device_id());
    assert_eq!(
        &opened.sender_exchange_public_key,
        alice.device_info().exchange_public_key()
    );
    assert_eq!(
        opened.inner_payload, alert,
        "the exact signed alert payload must survive the round trip"
    );
    // The recovered inner alert must still verify against the real identities.
    match VersionedPayload::decode(&opened.inner_payload).expect("decode inner") {
        VersionedPayload::Alert(a) => assert!(
            a.verify(alice.signing_public_key(), bob.signing_public_key()),
            "inner alert signature must verify after genesis transport"
        ),
        other => panic!("expected an alert payload, got {other:?}"),
    }
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn genesis_open_rejects_shared_key_only_when_identities_swapped() {
    // The envelope signature binds both identities; opening under a wrong
    // sender identity must fail even though shared_key alone decrypts.
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let mallory = Identity::create("Mallory", 0);
    let shared = SymmetricKey::generate();
    let broadcast = sender_broadcast(&alice);
    let alert = signed_alert_bytes(&alice, &bob);

    let (msg, _) = GenesisEnvelope::seal(
        &shared,
        &alice,
        bob.signing_public_key(),
        &broadcast,
        EPOCH,
        &alert,
    )
    .expect("seal");

    let err = GenesisEnvelope::open(
        &shared,
        mallory.signing_public_key(),
        bob.signing_public_key(),
        &msg,
    )
    .expect_err("opening under the wrong sender identity must fail");
    assert!(
        matches!(
            err,
            GenesisError::SignatureInvalid | GenesisError::Crypto(_)
        ),
        "wrong sender identity must not yield a valid open, got {err:?}"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn genesis_open_rejects_header_index_above_bound_before_decrypt() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let broadcast = sender_broadcast(&alice);
    let alert = signed_alert_bytes(&alice, &bob);

    let (mut msg, _) = GenesisEnvelope::seal(
        &shared,
        &alice,
        bob.signing_public_key(),
        &broadcast,
        EPOCH,
        &alert,
    )
    .expect("seal");
    msg.message_index = GENESIS_MAX_CHAIN_INDEX + 1;

    let err = GenesisEnvelope::open(
        &shared,
        alice.signing_public_key(),
        bob.signing_public_key(),
        &msg,
    )
    .expect_err("an out-of-bound chain index must be rejected before key derivation");
    assert!(
        matches!(err, GenesisError::ChainIndexTooHigh),
        "expected ChainIndexTooHigh, got {err:?}"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn genesis_open_fails_for_a_different_shared_key() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let wrong = SymmetricKey::generate();
    let broadcast = sender_broadcast(&alice);
    let alert = signed_alert_bytes(&alice, &bob);

    let (msg, _) = GenesisEnvelope::seal(
        &shared,
        &alice,
        bob.signing_public_key(),
        &broadcast,
        EPOCH,
        &alert,
    )
    .expect("seal");

    let err = GenesisEnvelope::open(
        &wrong,
        alice.signing_public_key(),
        bob.signing_public_key(),
        &msg,
    )
    .expect_err("a different shared_key must not open the genesis message");
    assert!(
        matches!(err, GenesisError::Crypto(_)),
        "expected a crypto/decrypt failure, got {err:?}"
    );
}
