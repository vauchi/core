// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The registry names each device's own signing key from its first version:
//! a joining device sends its public key in the link request (ADR-079, SC-62,
//! vauchi/private#970).

use vauchi_core::Vauchi;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkQR, DeviceLinkRequest, DeviceLinkResponder, DeviceLinkResponse,
    ExchangeError, ProximityProof,
};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, Identity};

const SEED: [u8; 32] = [0x42; 32];

fn now() -> u64 {
    vauchi_core::clock::SystemClock::shared().unix_seconds()
}

fn public_key_of_stored_key(vauchi: &Vauchi) -> [u8; 32] {
    *vauchi
        .storage()
        .device()
        .load_or_create_device_signing_key()
        .unwrap()
        .public_key()
        .as_bytes()
}

struct Host {
    identity: Identity,
    initiator: DeviceLinkInitiator,
    registry: DeviceRegistry,
    own_key: [u8; 32],
}

fn host() -> Host {
    let identity = Identity::from_device_link(SEED, "Alice".into(), 0, "Host".into(), now());
    let own_key = [0x07; 32];
    let own_key = *SigningKeyPair::from_seed(&own_key).public_key().as_bytes();
    let registry = DeviceRegistry::new(
        identity
            .device_info()
            .to_registered(&SEED)
            .with_signing_key(own_key),
        identity.signing_keypair(),
    );
    let initiator = DeviceLinkInitiator::new(SEED, &identity, registry.clone(), now());
    Host {
        identity,
        initiator,
        registry,
        own_key,
    }
}

fn responder_for(initiator: &DeviceLinkInitiator) -> DeviceLinkResponder {
    let qr = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string()).unwrap();
    DeviceLinkResponder::from_qr(qr, "My Phone".into(), now()).unwrap()
}

fn confirm(
    initiator: &DeviceLinkInitiator,
    encrypted_request: &[u8],
) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
    let (confirmation, request) = initiator.prepare_confirmation(encrypted_request)?;
    let proof = ProximityProof::manual_confirmation(
        initiator.qr().link_key(),
        &confirmation.confirmation_code,
        now(),
    );
    initiator.confirm_link(&request, &proof, now())
}

fn request_naming(initiator: &DeviceLinkInitiator, key: [u8; 32]) -> Vec<u8> {
    DeviceLinkRequest::new("My Phone".into(), key, now())
        .encrypt(initiator.qr().link_key())
        .unwrap()
}

fn joined() -> (Vauchi, DeviceRegistry, u32, [u8; 32]) {
    let host = host();
    let mut responder = responder_for(&host.initiator);
    let request = responder.create_request(now()).unwrap();
    let (encrypted, registry, new_device) = confirm(&host.initiator, &request).unwrap();
    let response = responder.process_response(&encrypted).unwrap();

    let mut joiner = Vauchi::in_memory().unwrap();
    joiner
        .adopt_device_link_response(&response, "My Phone".into())
        .unwrap();
    (
        joiner,
        registry,
        response.device_index(),
        *new_device.device_id(),
    )
}

// @internal
#[test]
fn a_joined_device_stores_the_key_the_registry_names_for_it() {
    let (joiner, registry, device_index, device_id) = joined();

    assert_eq!(device_index, 1);
    let entry = registry.find_device(&device_id).unwrap();
    assert_eq!(
        entry.signing_public_key,
        Some(public_key_of_stored_key(&joiner))
    );
}

// @internal
#[test]
fn a_joined_device_keeps_the_registry_it_was_given() {
    let (joiner, registry, _, device_id) = joined();

    let stored = joiner
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.find_device(&device_id).unwrap().signing_public_key,
        registry.find_device(&device_id).unwrap().signing_public_key
    );
}

// @internal
#[test]
fn linking_leaves_the_hosts_own_key_in_the_registry() {
    let host = host();
    let mut responder = responder_for(&host.initiator);
    let request = responder.create_request(now()).unwrap();

    let (_, registry, _) = confirm(&host.initiator, &request).unwrap();

    let entry = registry
        .find_device(host.identity.device_info().device_id())
        .unwrap();
    assert_eq!(entry.signing_public_key, Some(host.own_key));
    assert!(registry.verify(&host.identity.signing_keypair().public_key()));
}

// @internal
#[test]
fn two_joined_devices_hold_different_keys() {
    let first = joined();
    let second = joined();

    assert_ne!(
        public_key_of_stored_key(&first.0),
        public_key_of_stored_key(&second.0)
    );
}

// @internal
#[test]
fn a_retried_request_names_the_same_key() {
    let host = host();
    let link_key = *host.initiator.qr().link_key();
    let mut responder = responder_for(&host.initiator);

    let first = DeviceLinkRequest::decrypt(&responder.create_request(now()).unwrap(), &link_key);
    let second = DeviceLinkRequest::decrypt(&responder.create_request(now()).unwrap(), &link_key);

    assert_eq!(
        first.unwrap().device_signing_public_key,
        second.unwrap().device_signing_public_key
    );
}

// @internal
#[test]
fn a_host_refuses_a_key_no_device_could_use_or_own() {
    let host = host();
    let identity_key = *host.identity.signing_keypair().public_key().as_bytes();
    let mut small_order_identity_point = [0u8; 32];
    small_order_identity_point[0] = 1;
    let mut off_curve = [0u8; 32];
    off_curve[0] = 2;
    let mut second_encoding = [0xff; 32];
    second_encoding[0] = 0xf0;
    second_encoding[31] = 0x7f;

    for (case, key) in [
        ("zero, a small-order point", [0u8; 32]),
        ("the identity point", small_order_identity_point),
        ("not a curve point", off_curve),
        ("a second encoding of y = 3", second_encoding),
        ("all ones, non-canonical", [0xff; 32]),
        ("the identity key", identity_key),
        ("a registered device's key", host.own_key),
    ] {
        let request = request_naming(&host.initiator, key);

        let result = confirm(&host.initiator, &request);

        assert!(
            matches!(result, Err(ExchangeError::DeviceKeyRefused)),
            "{case} was not refused"
        );
    }
}

// @internal
#[test]
fn a_host_accepts_a_usable_key_and_names_it() {
    let host = host();
    let key = *SigningKeyPair::from_seed(&[0x09; 32])
        .public_key()
        .as_bytes();
    let request = request_naming(&host.initiator, key);

    let (_, registry, new_device) = confirm(&host.initiator, &request).unwrap();

    assert_eq!(
        registry
            .find_device(new_device.device_id())
            .unwrap()
            .signing_public_key,
        Some(key)
    );
}

fn response_with_registry(host: &Host, registry: DeviceRegistry) -> Vec<u8> {
    DeviceLinkResponse::new(SEED, "Alice".into(), 1, registry)
        .encrypt(host.initiator.qr().link_key())
        .unwrap()
}

// @internal
#[test]
fn a_joiner_refuses_a_registry_that_names_another_key_for_it() {
    let host = host();
    let mut responder = responder_for(&host.initiator);
    responder.create_request(now()).unwrap();
    let other_key = *SigningKeyPair::from_seed(&[0x0a; 32])
        .public_key()
        .as_bytes();
    let new_device = DeviceInfo::derive(&SEED, 1, "My Phone".into(), now());
    let mut registry = host.registry.clone();
    registry
        .add_device(
            new_device.to_registered(&SEED).with_signing_key(other_key),
            host.identity.signing_keypair(),
        )
        .unwrap();

    let result = responder.process_response(&response_with_registry(&host, registry));

    assert!(matches!(result, Err(ExchangeError::DeviceKeyRefused)));
}

// @internal
#[test]
fn a_joiner_refuses_a_registry_with_no_key_for_it() {
    let host = host();
    let mut responder = responder_for(&host.initiator);
    responder.create_request(now()).unwrap();
    let new_device = DeviceInfo::derive(&SEED, 1, "My Phone".into(), now());
    let mut keyless = host.registry.clone();
    keyless
        .add_device(
            new_device.to_registered(&SEED),
            host.identity.signing_keypair(),
        )
        .unwrap();

    let result = responder.process_response(&response_with_registry(&host, keyless));

    assert!(matches!(result, Err(ExchangeError::DeviceKeyRefused)));
}

// @internal
#[test]
fn a_joiner_refuses_a_registry_with_no_entry_for_it() {
    let host = host();
    let mut responder = responder_for(&host.initiator);
    responder.create_request(now()).unwrap();
    let registry = host.registry.clone();

    let result = responder.process_response(&response_with_registry(&host, registry));

    assert!(matches!(result, Err(ExchangeError::DeviceKeyRefused)));
}

// @internal
#[test]
fn adopting_a_response_with_no_device_key_leaves_no_identity() {
    let host = host();
    let response = DeviceLinkResponse::new(SEED, "Alice".into(), 1, host.registry.clone());
    let mut joiner = Vauchi::in_memory().unwrap();

    let result = joiner.adopt_device_link_response(&response, "My Phone".into());

    assert!(result.is_err());
    assert!(!joiner.has_identity());
    assert_eq!(joiner.storage().identity().load_identity().unwrap(), None);
}

// @internal
#[test]
fn adopting_a_key_keeps_it_and_repeats_harmlessly() {
    let vauchi = Vauchi::in_memory().unwrap();
    let seed = zeroize::Zeroizing::new([0x0b; 32]);
    let expected = *SigningKeyPair::from_seed(&seed).public_key().as_bytes();

    vauchi
        .storage()
        .device()
        .adopt_device_signing_key(&seed)
        .unwrap();
    vauchi
        .storage()
        .device()
        .adopt_device_signing_key(&seed)
        .unwrap();

    assert_eq!(public_key_of_stored_key(&vauchi), expected);
}

// @internal
#[test]
fn adopting_a_second_key_is_refused_and_the_first_stays() {
    let vauchi = Vauchi::in_memory().unwrap();
    let existing = public_key_of_stored_key(&vauchi);

    let result = vauchi
        .storage()
        .device()
        .adopt_device_signing_key(&zeroize::Zeroizing::new([0x0c; 32]));

    assert!(result.is_err());
    assert_eq!(public_key_of_stored_key(&vauchi), existing);
}

// @internal
#[test]
fn a_first_device_names_its_stored_key_in_the_registry_it_offers() {
    let wb = crate::common::helpers::create_vauchi_with_identity("Alice");
    let identity = wb.identity().unwrap();

    let registry = wb.storage().device().own_device_registry(identity).unwrap();

    let entry = registry
        .find_device(identity.device_info().device_id())
        .unwrap();
    assert_eq!(
        entry.signing_public_key,
        Some(public_key_of_stored_key(&wb))
    );
    assert!(registry.verify(&identity.signing_keypair().public_key()));
}

// @internal
#[test]
fn a_saved_registry_is_returned_as_saved() {
    let host = host();
    let vauchi = Vauchi::in_memory().unwrap();
    let saved = host.registry.clone();
    vauchi
        .storage()
        .device()
        .save_device_registry(&saved)
        .unwrap();

    let registry = vauchi
        .storage()
        .device()
        .own_device_registry(&host.identity)
        .unwrap();

    assert_eq!(registry.to_json(), saved.to_json());
}

// @internal
#[test]
fn a_link_started_on_a_first_device_offers_its_key() {
    let wb = crate::common::helpers::create_vauchi_with_identity("Alice");
    let initiator = wb.device_link_initiator().unwrap();
    let mut responder = responder_for(&initiator);
    let request = responder.create_request(now()).unwrap();

    let (_, registry, _) = confirm(&initiator, &request).unwrap();

    let device_id = *wb.identity().unwrap().device_info().device_id();
    assert_eq!(
        registry.find_device(&device_id).unwrap().signing_public_key,
        Some(public_key_of_stored_key(&wb))
    );
}
