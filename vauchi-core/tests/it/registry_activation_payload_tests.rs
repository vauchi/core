// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 bilateral registry activation — wire payloads (ADR-064 Amendment
//! 2026-07-25).
//!
//! `RegistryPush` (0x05) carries the sender's identity-signed
//! `RegistryBroadcast` to a contact over the existing ratcheted channel;
//! `RegistryAck` (0x06) confirms the received version and optionally carries
//! the responder's own broadcast back. Both are structural containers — the
//! broadcast's own Ed25519 signature is verified at persist time against the
//! contact's known identity key, not here.

use proptest::prelude::*;
use vauchi_core::identity::Identity;
use vauchi_core::sync::delta::{DeltaError, VersionedPayload};
use vauchi_core::sync::registry_activation::{
    MAX_BROADCAST_JSON_BYTES, RegistryAckPayload, RegistryPushPayload,
};

fn test_broadcast_json() -> String {
    let identity = Identity::create("Push Tester", 0);
    let registry = identity.initial_device_registry();
    let broadcast = vauchi_core::identity::RegistryBroadcast::new(
        &registry,
        identity.signing_keypair(),
        1_753_000_000,
    );
    broadcast.to_json()
}

// @internal
#[test]
fn push_payload_roundtrips_through_versioned_payload() {
    let json = test_broadcast_json();
    let payload = RegistryPushPayload::new([7u8; 32], json.clone().into_bytes()).expect("payload");

    let wire = VersionedPayload::encode_registry_push(&payload);
    assert_eq!(wire[0], 0x05, "RegistryPush owns version byte 0x05");

    match VersionedPayload::decode(&wire).expect("decode") {
        VersionedPayload::RegistryPush(decoded) => {
            assert_eq!(decoded.push_nonce(), &[7u8; 32]);
            assert_eq!(decoded.broadcast_json(), json.as_bytes());
        }
        other => panic!("expected RegistryPush, got {other:?}"),
    }
}

// @internal
#[test]
fn ack_payload_roundtrips_with_and_without_broadcast_echo() {
    let json = test_broadcast_json();

    let with_echo =
        RegistryAckPayload::new([9u8; 32], 3, Some(json.clone().into_bytes())).expect("ack");
    let wire = VersionedPayload::encode_registry_ack(&with_echo);
    assert_eq!(wire[0], 0x06, "RegistryAck owns version byte 0x06");
    match VersionedPayload::decode(&wire).expect("decode") {
        VersionedPayload::RegistryAck(decoded) => {
            assert_eq!(decoded.push_nonce(), &[9u8; 32]);
            assert_eq!(decoded.acked_version(), 3);
            assert_eq!(decoded.broadcast_json(), Some(json.as_bytes()));
        }
        other => panic!("expected RegistryAck, got {other:?}"),
    }

    let without_echo = RegistryAckPayload::new([9u8; 32], 7, None).expect("ack");
    let wire = VersionedPayload::encode_registry_ack(&without_echo);
    match VersionedPayload::decode(&wire).expect("decode") {
        VersionedPayload::RegistryAck(decoded) => {
            assert_eq!(decoded.acked_version(), 7);
            assert_eq!(decoded.broadcast_json(), None);
        }
        other => panic!("expected RegistryAck, got {other:?}"),
    }
}

// @internal
#[test]
fn push_rejects_oversized_broadcast_at_construction_and_decode() {
    let oversized = vec![b'x'; MAX_BROADCAST_JSON_BYTES + 1];
    assert!(matches!(
        RegistryPushPayload::new([1u8; 32], oversized.clone()),
        Err(DeltaError::InvalidPayload(_))
    ));

    // A hostile peer can still put oversized bytes on the wire directly —
    // the decode boundary must enforce the same ceiling (DC-01).
    let mut wire = vec![0x05];
    wire.extend_from_slice(&[1u8; 32]);
    wire.extend_from_slice(&oversized);
    assert!(matches!(
        VersionedPayload::decode(&wire),
        Err(DeltaError::InvalidPayload(_))
    ));
}

// @internal
#[test]
fn ack_rejects_oversized_echo_at_construction_and_decode() {
    let oversized = vec![b'x'; MAX_BROADCAST_JSON_BYTES + 1];
    assert!(matches!(
        RegistryAckPayload::new([1u8; 32], 1, Some(oversized.clone())),
        Err(DeltaError::InvalidPayload(_))
    ));

    let mut wire = vec![0x06];
    wire.extend_from_slice(&[1u8; 32]);
    wire.extend_from_slice(&1u64.to_be_bytes());
    wire.push(1);
    wire.extend_from_slice(&oversized);
    assert!(matches!(
        VersionedPayload::decode(&wire),
        Err(DeltaError::InvalidPayload(_))
    ));
}

// @internal
#[test]
fn truncated_payloads_fail_closed() {
    // Push shorter than its 32-byte nonce prefix.
    let mut wire = vec![0x05];
    wire.extend_from_slice(&[1u8; 16]);
    assert!(VersionedPayload::decode(&wire).is_err());

    // Ack shorter than nonce(32) + version(8) + flag(1).
    let mut wire = vec![0x06];
    wire.extend_from_slice(&[1u8; 32]);
    wire.extend_from_slice(&[0u8; 4]);
    assert!(VersionedPayload::decode(&wire).is_err());

    // Ack whose echo flag promises bytes that are not there.
    let mut wire = vec![0x06];
    wire.extend_from_slice(&[1u8; 32]);
    wire.extend_from_slice(&1u64.to_be_bytes());
    wire.push(1);
    assert!(VersionedPayload::decode(&wire).is_err());
}

// @internal
#[test]
fn push_rejects_bytes_that_are_not_a_registry_broadcast() {
    // Structural DC-01 gate: the carried JSON must parse as a
    // RegistryBroadcast — arbitrary JSON is rejected before it can reach
    // storage.
    let mut wire = vec![0x05];
    wire.extend_from_slice(&[1u8; 32]);
    wire.extend_from_slice(b"{\"not\":\"a broadcast\"}");
    assert!(matches!(
        VersionedPayload::decode(&wire),
        Err(DeltaError::InvalidPayload(_))
    ));
}

// @internal
#[test]
fn unknown_future_version_byte_still_fails_closed() {
    // The degrade path a pre-F4 decoder takes for 0x05/0x06 — and this
    // decoder for any later variant: a clean UnknownPayloadVersion, never a
    // panic or misparse (mixed-version pin, F4 plan §Mixed-version degrade).
    assert!(matches!(
        VersionedPayload::decode(&[0x7F, 1, 2, 3]),
        Err(DeltaError::UnknownPayloadVersion(0x7F))
    ));
}

proptest! {
    // @internal
    #[test]
    fn push_wire_form_is_stable_over_arbitrary_nonces(nonce in prop::array::uniform32(any::<u8>())) {
        let json = test_broadcast_json();
        let payload = RegistryPushPayload::new(nonce, json.into_bytes()).expect("payload");
        let wire = VersionedPayload::encode_registry_push(&payload);
        let decoded = match VersionedPayload::decode(&wire).expect("decode") {
            VersionedPayload::RegistryPush(p) => p,
            other => panic!("expected RegistryPush, got {other:?}"),
        };
        prop_assert_eq!(VersionedPayload::encode_registry_push(&decoded), wire);
    }

    // @internal
    #[test]
    fn ack_wire_form_is_stable_over_versions_and_echo(
        nonce in prop::array::uniform32(any::<u8>()),
        version in any::<u64>(),
        echo in any::<bool>(),
    ) {
        let json = test_broadcast_json();
        let echo_bytes = echo.then(|| json.into_bytes());
        let payload = RegistryAckPayload::new(nonce, version, echo_bytes).expect("ack");
        let wire = VersionedPayload::encode_registry_ack(&payload);
        let decoded = match VersionedPayload::decode(&wire).expect("decode") {
            VersionedPayload::RegistryAck(p) => p,
            other => panic!("expected RegistryAck, got {other:?}"),
        };
        prop_assert_eq!(decoded.acked_version(), version);
        prop_assert_eq!(VersionedPayload::encode_registry_ack(&decoded), wire);
    }
}
