// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Malicious compressed frames need the private wire-building seam; assertions
//! exercise GenesisEnvelope::open and its public authentication/admission contract.

use super::*;
use crate::sync::delta::VersionedPayload;
use crate::sync::registry_activation::RegistryPushPayload;
use crate::sync::safety_alert::{AlertKind, SafetyAlertPayload};

const EPOCH: u64 = 20_100;

fn forged_message(
    shared: &SymmetricKey,
    sender: &Identity,
    receiver: &Identity,
    broadcast: &RegistryBroadcast,
    payload: &[u8],
    valid_signature: bool,
) -> RatchetMessage {
    let root = genesis_root(
        shared,
        sender.signing_public_key(),
        receiver.signing_public_key(),
    );
    let responder = genesis_responder_keypair(
        shared,
        sender.signing_public_key(),
        receiver.signing_public_key(),
    );
    let mut session =
        DoubleRatchetState::initialize_initiator(&root, *responder.public_key()).unwrap();
    let header = MessageHeader {
        dh_public: session.our_public_key(),
        dh_generation: 0,
        message_index: 0,
        previous_chain_length: 0,
    };
    let json = broadcast.to_json().into_bytes();
    let signable = signable_bytes(
        sender.signing_public_key(),
        receiver.signing_public_key(),
        EPOCH,
        &header,
        sender.device_id(),
        sender.device_info().exchange_public_key(),
        &json,
        payload,
    );
    let signature = if valid_signature {
        *sender.sign(&signable).as_bytes()
    } else {
        [0; 64]
    };
    let canonical = encode_envelope(
        EPOCH,
        sender.device_id(),
        sender.device_info().exchange_public_key(),
        &signature,
        &json,
        payload,
    );
    assert!(
        canonical.len() > 4092,
        "the malicious frame must exercise compression"
    );
    let wire = control_codec::pack(canonical, true).unwrap();
    assert_eq!(wire.len(), 4092);
    session.encrypt(&wire).unwrap()
}

// @internal
#[test]
fn compressed_control_still_requires_the_identity_signature() {
    let sender = Identity::create("Alice", 0);
    let receiver = Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let broadcast = RegistryBroadcast::new(
        &sender.initial_device_registry(),
        sender.signing_keypair(),
        0,
    );
    let mut raw = broadcast.to_json().into_bytes();
    raw.resize(8192, b' ');
    let payload =
        VersionedPayload::encode_registry_push(&RegistryPushPayload::new([7; 32], raw).unwrap());
    let good = forged_message(&shared, &sender, &receiver, &broadcast, &payload, true);
    let opened = GenesisEnvelope::open(
        &shared,
        sender.signing_public_key(),
        receiver.signing_public_key(),
        &good,
    )
    .unwrap();
    assert_eq!(opened.inner_payload, payload);
    assert_eq!(
        opened.sender_registry_broadcast_json,
        broadcast.to_json().as_bytes()
    );
    let bad = forged_message(&shared, &sender, &receiver, &broadcast, &payload, false);
    assert_eq!(good.ciphertext.len(), bad.ciphertext.len());
    let error = GenesisEnvelope::open(
        &shared,
        sender.signing_public_key(),
        receiver.signing_public_key(),
        &bad,
    )
    .unwrap_err();
    assert!(matches!(error, GenesisError::SignatureInvalid), "{error:?}");
}

// @internal
#[test]
fn even_an_identity_signed_compressed_private_alert_is_not_admitted() {
    let sender = Identity::create("Alice", 0);
    let receiver = Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let broadcast = RegistryBroadcast::new(
        &sender.initial_device_registry(),
        sender.signing_keypair(),
        0,
    );
    let alert = SafetyAlertPayload::new(
        AlertKind::Emergency,
        "p".repeat(4096),
        7_000,
        None,
        [9; 32],
        &sender,
        receiver.signing_public_key(),
    )
    .unwrap();
    let payload = VersionedPayload::encode_alert(&alert);
    let message = forged_message(&shared, &sender, &receiver, &broadcast, &payload, true);
    let error = GenesisEnvelope::open(
        &shared,
        sender.signing_public_key(),
        receiver.signing_public_key(),
        &message,
    )
    .unwrap_err();
    assert!(matches!(error, GenesisError::InvalidFormat), "{error:?}");
    let error = GenesisEnvelope::seal(
        &shared,
        &sender,
        receiver.signing_public_key(),
        &broadcast,
        EPOCH,
        &payload,
    )
    .unwrap_err();
    assert!(matches!(error, GenesisError::TooLarge), "{error:?}");
}
