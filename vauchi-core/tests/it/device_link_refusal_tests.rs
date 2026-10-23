// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A host that ends the ceremony tells the joiner why, on the channel the
//! response would have used (vauchi/private#780). Only someone holding the
//! QR's link key can write one, and it never reads as a response.

use proptest::prelude::*;
use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkQR, DeviceLinkRefusalReason, DeviceLinkResponder, ProximityProof,
};
use vauchi_core::identity::{DeviceRegistry, Identity};

const NOW: u64 = 1_800_000_000;

fn ceremony(seed: u8) -> (DeviceLinkInitiator, DeviceLinkResponder) {
    let master_seed = [seed; 32];
    let identity = Identity::create("Alice", NOW);
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&master_seed),
        identity.signing_keypair(),
    );
    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry, NOW);
    let scanned = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string())
        .expect("a fresh QR parses");
    let responder = DeviceLinkResponder::from_qr(scanned, "New Phone".to_string(), NOW)
        .expect("a fresh QR is accepted");
    (initiator, responder)
}

const REASONS: [DeviceLinkRefusalReason; 4] = [
    DeviceLinkRefusalReason::Declined,
    DeviceLinkRefusalReason::ConfirmationTimedOut,
    DeviceLinkRefusalReason::DeviceNameTaken,
    DeviceLinkRefusalReason::Cancelled,
];

// @internal
#[test]
fn the_joiner_reads_each_reason_the_host_refused_with() {
    let (initiator, responder) = ceremony(0x42);
    for reason in REASONS {
        let refusal = initiator.refuse(reason).expect("a refusal encrypts");
        assert_eq!(responder.process_refusal(&refusal).ok(), Some(reason));
    }
}

// @internal
#[test]
fn a_refusal_from_another_ceremony_is_not_believed() {
    let (other_host, _) = ceremony(0x17);
    let (_, responder) = ceremony(0x42);
    let foreign = other_host
        .refuse(DeviceLinkRefusalReason::Declined)
        .expect("a refusal encrypts");
    assert!(responder.process_refusal(&foreign).is_err());
}

// @internal
#[test]
fn a_refusal_never_reads_as_a_response_nor_a_response_as_a_refusal() {
    let (initiator, mut responder) = ceremony(0x42);
    let refusal = initiator
        .refuse(DeviceLinkRefusalReason::Declined)
        .expect("a refusal encrypts");
    assert!(responder.process_response(&refusal).is_err());

    let request = responder.create_request(NOW).expect("request");
    let (confirmation, request) = initiator
        .prepare_confirmation(&request)
        .expect("a fresh name is accepted");
    let proof = ProximityProof::manual_confirmation(
        initiator.qr().link_key(),
        &confirmation.confirmation_code,
        NOW,
    );
    let (response, _, _) = initiator
        .confirm_link(&request, &proof, NOW)
        .expect("a confirmed link answers");
    assert!(responder.process_refusal(&response).is_err());
}

// @internal
#[test]
fn a_tampered_refusal_is_rejected() {
    let (initiator, responder) = ceremony(0x42);
    let mut refusal = initiator
        .refuse(DeviceLinkRefusalReason::DeviceNameTaken)
        .expect("a refusal encrypts");
    let last = refusal.len() - 1;
    refusal[last] ^= 0x01;
    assert!(responder.process_refusal(&refusal).is_err());
}

proptest! {
    // @internal
    #[test]
    fn arbitrary_bytes_are_never_a_refusal(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let (_, responder) = ceremony(0x42);
        prop_assert!(responder.process_refusal(&bytes).is_err());
    }
}
