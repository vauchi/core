// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M5 B3 Slice 1: `Vauchi::adopt_device_link_response` — a fresh
//! (identity-less) instance becomes a new device of an existing identity
//! by adopting a decrypted device-link response. Two-party crypto join
//! (initiator ⇄ responder), mirroring `device_link_proximity_tests`.

use vauchi_core::api::Vauchi;
use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkQR, DeviceLinkResponder, DeviceLinkResponse, ProximityProof,
};
use vauchi_core::identity::{DeviceRegistry, Identity};

fn now() -> u64 {
    vauchi_core::clock::SystemClock::shared().unix_seconds()
}

/// Runs the full initiator⇄responder crypto dance for a known master seed
/// and returns the decrypted response the joining device would adopt.
fn make_response(master_seed: [u8; 32], display_name: &str) -> DeviceLinkResponse {
    let initiator_identity = Identity::from_device_link(
        master_seed,
        display_name.into(),
        0,
        "Initiator".into(),
        now(),
    );
    let registry = DeviceRegistry::new(
        initiator_identity.device_info().to_registered(&master_seed),
        initiator_identity.signing_keypair(),
    );
    let initiator = DeviceLinkInitiator::new(master_seed, &initiator_identity, registry, now());

    let qr = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string()).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(qr, "My Phone".into(), now()).unwrap();
    let encrypted_request = responder.create_request(now()).unwrap();

    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();
    let proof = ProximityProof::Ultrasonic {
        challenge_response: initiator.proximity_challenge(),
        verified_at: now(),
    };
    let (encrypted_response, _registry, _new_device) =
        initiator.confirm_link(&request, &proof, now()).unwrap();

    responder.process_response(&encrypted_response).unwrap()
}

// @internal
#[test]
fn adopt_device_link_response_creates_matching_identity() {
    let master_seed = [0x42u8; 32];
    // The identity every device of this seed resolves to.
    let reference =
        Identity::from_device_link(master_seed, "Alice".into(), 0, "Initiator".into(), now());

    let response = make_response(master_seed, "Alice");
    // The joining device is device index 1 of the same identity.
    assert_eq!(response.device_index(), 1);
    assert_eq!(response.display_name(), "Alice");

    let mut joiner = Vauchi::in_memory().unwrap();
    assert!(!joiner.has_identity());

    joiner
        .adopt_device_link_response(&response, "My Phone".into())
        .expect("adopt succeeds on a fresh instance");

    assert!(joiner.has_identity(), "identity must be set after adopt");
    assert_eq!(
        joiner.public_id().unwrap(),
        reference.public_id(),
        "the joined device shares the initiator's identity"
    );
}

/// Same crypto dance, but the initiator confirms *with* a sync payload —
/// exercises the adopt path's `apply_full_sync` branch.
fn make_response_with_sync(
    master_seed: [u8; 32],
    synced_own_card_name: &str,
) -> DeviceLinkResponse {
    use vauchi_core::contact_card::ContactCard;
    use vauchi_core::sync::DeviceSyncPayload;

    let initiator_identity =
        Identity::from_device_link(master_seed, "Alice".into(), 0, "Initiator".into(), now());
    let registry = DeviceRegistry::new(
        initiator_identity.device_info().to_registered(&master_seed),
        initiator_identity.signing_keypair(),
    );
    let initiator = DeviceLinkInitiator::new(master_seed, &initiator_identity, registry, now());

    let qr = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string()).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(qr, "My Phone".into(), now()).unwrap();
    let encrypted_request = responder.create_request(now()).unwrap();

    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();
    let proof = ProximityProof::Ultrasonic {
        challenge_response: initiator.proximity_challenge(),
        verified_at: now(),
    };

    let synced_card = ContactCard::new(synced_own_card_name);
    let sync_json = serde_json::to_string(&DeviceSyncPayload::new(&[], &synced_card, 1)).unwrap();
    let (encrypted_response, _registry, _new_device) = initiator
        .confirm_link_with_sync(&request, &sync_json, &proof, now())
        .unwrap();

    responder.process_response(&encrypted_response).unwrap()
}

// @internal
#[test]
fn adopt_applies_sync_payload_own_card() {
    let response = make_response_with_sync([0x42u8; 32], "Alice From Sync");
    assert!(!response.sync_payload_json().is_empty());

    let mut joiner = Vauchi::in_memory().unwrap();
    joiner
        .adopt_device_link_response(&response, "My Phone".into())
        .expect("adopt succeeds");

    // The synced own-card (distinctive name) was applied — not the default
    // card the empty-sync path would seed from the identity's "Alice".
    let card = joiner
        .own_card()
        .expect("own_card query")
        .expect("own card present");
    assert_eq!(card.display_name(), "Alice From Sync");
}

// @internal
#[test]
fn adopt_rejects_when_identity_already_exists() {
    let response = make_response([0x42u8; 32], "Alice");
    let mut joiner = Vauchi::in_memory().unwrap();
    joiner.create_identity("Bob").unwrap();

    let err = joiner
        .adopt_device_link_response(&response, "My Phone".into())
        .expect_err("adopt must refuse to overwrite an existing identity");
    assert!(
        format!("{err:?}").contains("AlreadyInitialized"),
        "expected AlreadyInitialized, got {err:?}"
    );
}
