// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The new device joins the identity its scanned QR names, and no other
//! (vauchi/private#789). Anyone holding a copy of the link QR holds the
//! link key, so the key alone cannot vouch for the seed it decrypts.

use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkQR, DeviceLinkResponder, DeviceLinkResponse, ExchangeError,
    ProximityProof,
};
use vauchi_core::identity::{DeviceRegistry, Identity};

fn now() -> u64 {
    vauchi_core::clock::SystemClock::shared().unix_seconds()
}

fn identity_and_registry(master_seed: [u8; 32], name: &str) -> (Identity, DeviceRegistry) {
    let identity = Identity::from_device_link(master_seed, name.into(), 0, "Laptop".into(), now());
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&master_seed),
        identity.signing_keypair(),
    );
    (identity, registry)
}

fn scan(initiator: &DeviceLinkInitiator) -> DeviceLinkResponder {
    let qr = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string()).unwrap();
    DeviceLinkResponder::from_qr(qr, "My Phone".into(), now()).unwrap()
}

// @internal
#[test]
fn a_seed_of_another_identity_under_the_link_key_is_refused() {
    let (alice, alice_registry) = identity_and_registry([0x11; 32], "Alice");
    let initiator = DeviceLinkInitiator::new([0x11; 32], &alice, alice_registry, now());
    let mut responder = scan(&initiator);
    responder.create_request(now()).unwrap();

    let attacker_seed = [0x66; 32];
    let (_, attacker_registry) = identity_and_registry(attacker_seed, "Alice");
    let forged = DeviceLinkResponse::new(attacker_seed, "Alice".into(), 1, attacker_registry)
        .encrypt(initiator.qr().link_key())
        .unwrap();

    assert!(matches!(
        responder.process_response(&forged),
        Err(ExchangeError::IdentityMismatch)
    ));
}

// @internal
#[test]
fn the_seed_of_the_identity_the_qr_names_is_adopted() {
    let master_seed = [0x11; 32];
    let (alice, registry) = identity_and_registry(master_seed, "Alice");
    let initiator = DeviceLinkInitiator::new(master_seed, &alice, registry, now());
    let mut responder = scan(&initiator);
    let encrypted_request = responder.create_request(now()).unwrap();

    let (confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();
    let proof = ProximityProof::manual_confirmation(
        initiator.qr().link_key(),
        &confirmation.confirmation_code,
        now(),
    );
    let (encrypted_response, _, _) = initiator.confirm_link(&request, &proof, now()).unwrap();

    let response = responder.process_response(&encrypted_response).unwrap();
    assert_eq!(response.master_seed(), &master_seed);
}
