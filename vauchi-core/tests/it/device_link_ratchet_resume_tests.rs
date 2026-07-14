// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device linking transfers signed peer topology, never live ratchet chains.

use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, Identity, RegistryBroadcast};
use vauchi_core::sync::DeviceLinkIntent;
use vauchi_core::{SigningKeyPair, Storage, X3DHKeyPair};

fn identity_and_registry(
    seed: [u8; 32],
    name: &str,
    device_index: u32,
) -> (Identity, DeviceRegistry) {
    let identity = Identity::from_device_link(
        seed,
        name.into(),
        device_index,
        format!("{name} device {device_index}"),
        1,
    );
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&seed),
        identity.signing_keypair(),
    );
    (identity, registry)
}

// @scenario: multi_device_sync :: Linked devices receive peer topology without shared ratchets
#[test]
fn linked_device_receives_signed_peer_registry_but_no_live_ratchet() {
    let source_storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let (alice1, alice_registry) = identity_and_registry([10u8; 32], "Alice", 0);
    let (bob, mut bob_registry) = identity_and_registry([20u8; 32], "Bob", 0);
    let bob_second = DeviceInfo::derive(&[20u8; 32], 1, "Bob laptop".into(), 1);
    bob_registry
        .add_device(bob_second.to_registered(&[20u8; 32]), bob.signing_keypair())
        .unwrap();

    let relationship = SymmetricKey::from_bytes([30u8; 32]);
    let bob_contact = Contact::from_exchange(
        *bob.signing_public_key(),
        ContactCard::new("Bob"),
        relationship,
        1,
    );
    let bob_id = bob_contact.id().to_string();
    source_storage
        .contacts()
        .save_contact(&bob_contact)
        .unwrap();
    let broadcast = RegistryBroadcast::new(&bob_registry, bob.signing_keypair(), 1);
    source_storage
        .device()
        .save_contact_device_registry(&bob_id, &broadcast, bob.signing_public_key(), u64::MAX)
        .unwrap();

    // Even if legacy replacement state exists, it is not exported.
    let ratchet = DoubleRatchetState::initialize_initiator(
        &SymmetricKey::generate(),
        *X3DHKeyPair::generate().public_key(),
    )
    .unwrap();
    source_storage
        .ratchets()
        .save_ratchet_state(&bob_id, &ratchet, true)
        .unwrap();

    let source = DeviceSyncOrchestrator::new(
        &source_storage,
        alice1.create_device_info(1),
        alice_registry.clone(),
    );
    let payload = source
        .create_full_sync_payload(DeviceLinkIntent::ReplaceDevice)
        .unwrap();
    assert_eq!(payload.contact_device_registries.len(), 1);

    let target_storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let alice2 =
        Identity::from_device_link([10u8; 32], "Alice".into(), 1, "Alice device 1".into(), 1);
    let mut target = DeviceSyncOrchestrator::new(
        &target_storage,
        alice2.create_device_info(1),
        alice_registry,
    );
    target.apply_full_sync(payload).unwrap();

    assert_eq!(
        target_storage
            .device()
            .load_contact_active_devices(&bob_id)
            .unwrap()
            .len(),
        2
    );
    assert!(
        target_storage
            .ratchets()
            .load_ratchet_state(&bob_id)
            .unwrap()
            .is_none(),
        "new owner device must bootstrap its own device-pair sessions"
    );
}

// @internal
#[test]
fn forged_peer_registry_in_owner_sync_is_rejected_atomically() {
    let source_storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let (alice, alice_registry) = identity_and_registry([40u8; 32], "Alice", 0);
    let contact_signing = SigningKeyPair::from_seed(&[50u8; 32]);
    let contact = Contact::from_exchange(
        *contact_signing.public_key().as_bytes(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        1,
    );
    let contact_id = contact.id().to_string();
    source_storage.contacts().save_contact(&contact).unwrap();
    let source = DeviceSyncOrchestrator::new(
        &source_storage,
        alice.create_device_info(1),
        alice_registry.clone(),
    );
    let mut payload = source
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();
    let attacker = SigningKeyPair::from_seed(&[60u8; 32]);
    let attacker_device = DeviceInfo::derive(&[61u8; 32], 0, "attacker".into(), 1);
    let attacker_registry =
        DeviceRegistry::new(attacker_device.to_registered(&[61u8; 32]), &attacker);
    payload
        .contact_device_registries
        .push(vauchi_core::sync::ContactDeviceRegistrySyncData {
            contact_id: contact_id.clone(),
            broadcast_json: RegistryBroadcast::new(&attacker_registry, &attacker, 1).to_json(),
        });

    let target_storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut target =
        DeviceSyncOrchestrator::new(&target_storage, alice.create_device_info(1), alice_registry);
    assert!(target.apply_full_sync(payload).is_err());
    assert!(
        target_storage
            .contacts()
            .load_contact(&contact_id)
            .unwrap()
            .is_none(),
        "failed registry verification must roll back the full sync"
    );
}
