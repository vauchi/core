// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The registry names each device's own signing key from its first version:
//! a joining device sends its public key in the link request (ADR-079, SC-62,
//! vauchi/private#970).

use vauchi_core::Vauchi;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkInitiatorRestored, DeviceLinkQR, DeviceLinkRequest,
    DeviceLinkResponder, DeviceLinkResponse, ExchangeError, ProximityProof,
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

const ZERO_KEY: [u8; 32] = [0u8; 32];

fn restored_initiator(host: &Host) -> DeviceLinkInitiatorRestored {
    let qr = DeviceLinkQR::from_data_string(&host.initiator.qr().to_data_string()).unwrap();
    DeviceLinkInitiatorRestored::new(SEED, &host.identity, host.registry.clone(), qr)
}

// @internal
#[test]
fn a_bad_key_is_refused_before_the_user_is_asked_to_confirm() {
    let host = host();
    let restored = restored_initiator(&host);
    let request = request_naming(&host.initiator, ZERO_KEY);

    assert!(matches!(
        host.initiator.prepare_confirmation(&request),
        Err(ExchangeError::DeviceKeyRefused)
    ));
    assert!(matches!(
        restored.prepare_confirmation(&request),
        Err(ExchangeError::DeviceKeyRefused)
    ));
}

// @internal
#[test]
fn a_key_changed_after_confirmation_is_refused() {
    let host = host();
    let restored = restored_initiator(&host);
    let usable = *SigningKeyPair::from_seed(&[0x0d; 32])
        .public_key()
        .as_bytes();
    let encrypted = request_naming(&host.initiator, usable);
    let link_key = *host.initiator.qr().link_key();

    let (confirmation, mut request) = host.initiator.prepare_confirmation(&encrypted).unwrap();
    request.device_signing_public_key = ZERO_KEY;
    let proof =
        ProximityProof::manual_confirmation(&link_key, &confirmation.confirmation_code, now());

    assert!(matches!(
        host.initiator.confirm_link(&request, &proof, now()),
        Err(ExchangeError::DeviceKeyRefused)
    ));
    assert!(matches!(
        restored.confirm_link(&request, &proof, now()),
        Err(ExchangeError::DeviceKeyRefused)
    ));
}

// @internal
#[test]
fn a_restored_host_names_the_joiners_key() {
    let host = host();
    let restored = restored_initiator(&host);
    let key = *SigningKeyPair::from_seed(&[0x0e; 32])
        .public_key()
        .as_bytes();
    let encrypted = request_naming(&host.initiator, key);
    let (confirmation, request) = restored.prepare_confirmation(&encrypted).unwrap();
    let proof = ProximityProof::manual_confirmation(
        host.initiator.qr().link_key(),
        &confirmation.confirmation_code,
        now(),
    );

    let (_, registry, new_device) = restored.confirm_link(&request, &proof, now()).unwrap();

    assert_eq!(
        registry
            .find_device(new_device.device_id())
            .unwrap()
            .signing_public_key,
        Some(key)
    );
}

fn usable_key(seed_byte: u8) -> [u8; 32] {
    *SigningKeyPair::from_seed(&[seed_byte; 32])
        .public_key()
        .as_bytes()
}

// @internal
#[test]
fn the_code_the_user_compares_covers_the_key() {
    let host = host();
    let mut request = DeviceLinkRequest::decrypt(
        &request_naming(&host.initiator, usable_key(0x10)),
        host.initiator.qr().link_key(),
    )
    .unwrap();
    let encrypted = request.encrypt(host.initiator.qr().link_key()).unwrap();
    let (confirmation, _) = host.initiator.prepare_confirmation(&encrypted).unwrap();

    request.device_signing_public_key = usable_key(0x11);
    let swapped = request.encrypt(host.initiator.qr().link_key()).unwrap();
    let (swapped_confirmation, _) = host.initiator.prepare_confirmation(&swapped).unwrap();

    assert_ne!(
        confirmation.confirmation_code,
        swapped_confirmation.confirmation_code
    );
}

// @internal
#[test]
fn a_usable_key_swapped_after_confirmation_is_refused() {
    let host = host();
    let restored = restored_initiator(&host);
    let encrypted = request_naming(&host.initiator, usable_key(0x12));
    let link_key = *host.initiator.qr().link_key();
    let (confirmation, mut request) = host.initiator.prepare_confirmation(&encrypted).unwrap();
    let proof =
        ProximityProof::manual_confirmation(&link_key, &confirmation.confirmation_code, now());

    request.device_signing_public_key = usable_key(0x13);

    assert!(matches!(
        host.initiator.confirm_link(&request, &proof, now()),
        Err(ExchangeError::ProximityNotVerified)
    ));
    assert!(matches!(
        restored.confirm_link(&request, &proof, now()),
        Err(ExchangeError::ProximityNotVerified)
    ));
}

// @internal
#[test]
fn both_devices_show_the_same_code() {
    let host = host();
    let mut responder = responder_for(&host.initiator);
    let request = responder.create_request(now()).unwrap();

    let (confirmation, _) = host.initiator.prepare_confirmation(&request).unwrap();

    assert_eq!(
        responder.compute_confirmation_code().unwrap(),
        confirmation.confirmation_code
    );
}

fn joiner_entry_key(host: &Host, responder: &mut DeviceLinkResponder) -> [u8; 32] {
    let encrypted = responder.create_request(now()).unwrap();
    DeviceLinkRequest::decrypt(&encrypted, host.initiator.qr().link_key())
        .unwrap()
        .device_signing_public_key
}

// @internal
#[test]
fn a_joiner_refuses_a_registry_where_its_entry_is_revoked() {
    let host = host();
    let mut responder = responder_for(&host.initiator);
    let key = joiner_entry_key(&host, &mut responder);
    let new_device = DeviceInfo::derive(&SEED, 1, "My Phone".into(), now());
    let signing = host.identity.signing_keypair();
    let mut registry = host.registry.clone();
    registry
        .add_device(
            new_device.to_registered(&SEED).with_signing_key(key),
            signing,
        )
        .unwrap();
    registry
        .revoke_device(new_device.device_id(), signing, now())
        .unwrap();

    let result = responder.process_response(&response_with_registry(&host, registry));

    assert!(matches!(result, Err(ExchangeError::DeviceKeyRefused)));
}

// @internal
#[test]
fn a_joiner_refuses_a_registry_the_identity_did_not_sign() {
    let host = host();
    let mut responder = responder_for(&host.initiator);
    let key = joiner_entry_key(&host, &mut responder);
    let new_device = DeviceInfo::derive(&SEED, 1, "My Phone".into(), now());
    let mut registry = host.registry.clone();
    registry
        .add_device_unsigned(new_device.to_registered(&SEED).with_signing_key(key))
        .unwrap();

    let result = responder.process_response(&response_with_registry(&host, registry));

    assert!(matches!(result, Err(ExchangeError::DeviceKeyRefused)));
}

fn response_with_sync(host: &Host, sync_json: &str) -> DeviceLinkResponse {
    let mut responder = responder_for(&host.initiator);
    let encrypted = responder.create_request(now()).unwrap();
    let (confirmation, request) = host.initiator.prepare_confirmation(&encrypted).unwrap();
    let proof = ProximityProof::manual_confirmation(
        host.initiator.qr().link_key(),
        &confirmation.confirmation_code,
        now(),
    );
    let (encrypted_response, _, _) = host
        .initiator
        .confirm_link_with_sync(&request, sync_json, &proof, now())
        .unwrap();
    responder.process_response(&encrypted_response).unwrap()
}

fn stored_device_key_rows(vauchi: &Vauchi) -> i64 {
    vauchi
        .storage()
        .connection()
        .query_row("SELECT COUNT(*) FROM device_signing_key", [], |row| {
            row.get(0)
        })
        .unwrap()
}

// @internal
#[test]
fn a_failed_sync_import_leaves_nothing_and_a_new_link_can_finish() {
    let host = host();
    let bad = response_with_sync(&host, "not a sync payload");
    let mut joiner = Vauchi::in_memory().unwrap();

    let failed = joiner.adopt_device_link_response(&bad, "My Phone".into());

    assert!(failed.is_err());
    assert!(!joiner.has_identity());
    assert_eq!(joiner.storage().identity().load_identity().unwrap(), None);
    assert_eq!(stored_device_key_rows(&joiner), 0);

    let good = response_with_sync(&host, &sync_payload());
    joiner
        .adopt_device_link_response(&good, "My Phone".into())
        .unwrap();
    assert!(joiner.has_identity());
    assert_eq!(stored_device_key_rows(&joiner), 1);
}

fn sync_payload() -> String {
    let card = vauchi_core::contact_card::ContactCard::new("Alice");
    serde_json::to_string(&vauchi_core::sync::DeviceSyncPayload::new(&[], &card, 1)).unwrap()
}

// @internal
#[test]
fn a_sync_payload_does_not_take_the_joiners_key_out_of_the_registry() {
    let host = host();
    let response = response_with_sync(&host, &sync_payload());
    let mut joiner = Vauchi::in_memory().unwrap();

    joiner
        .adopt_device_link_response(&response, "My Phone".into())
        .unwrap();

    let device_id = *joiner.identity().unwrap().device_info().device_id();
    let stored = joiner
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.find_device(&device_id).unwrap().signing_public_key,
        Some(public_key_of_stored_key(&joiner))
    );
}
