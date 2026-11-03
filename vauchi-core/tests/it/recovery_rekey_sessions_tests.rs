// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! After a rekey the contact's sessions start over with the new identity
//! (vauchi/private#796, D25): the old identity's device registry and
//! handshake state end with the old relationship, so the contact runs the
//! device-registry handshake with the new phone and sends nothing to the
//! old identity's devices.

use std::sync::Arc;
use std::time::SystemTime;

use vauchi_core::clock::FakeClock;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, RegistryBroadcast};
use vauchi_core::sync::registry_activation::ActivationTracker;
use vauchi_core::{ContactField, FieldType, Vauchi, VauchiError};

use crate::common::meeting::signing_pk;
use crate::common::recovered_phone::{World, recovered};

/// Alice's phone, restored; Dave holds her old identity's two devices and
/// a finished handshake with them, and has a field Alice may see.
fn dave_active_with_the_old_identity() -> (World, Vec<u8>) {
    let clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut phone = Vauchi::in_memory_with_clock(clock).unwrap();
    phone.create_identity("Alice, new phone").unwrap();
    let world = recovered(phone, None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    let blob = world.phone.rekey_offer_deliveries().unwrap()[0]
        .blob
        .clone();

    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();
    let signing = world.alice.identity().unwrap().signing_keypair();
    let device_seed = [44u8; 32];
    let first = DeviceInfo::derive(&device_seed, 0, "Alice phone".into(), 1);
    let second = DeviceInfo::derive(&device_seed, 1, "Alice laptop".into(), 1);
    let mut registry = DeviceRegistry::new(first.to_registered(&device_seed), signing);
    registry
        .add_device(second.to_registered(&device_seed), signing)
        .unwrap();
    let now = dave.storage().clock().unix_seconds();
    let broadcast = RegistryBroadcast::new(&registry, signing, now);
    dave.storage()
        .device()
        .save_contact_device_registry(&old_id, &broadcast, signing.public_key().as_bytes(), 60)
        .unwrap();
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([7u8; 32], 1);
    tracker.record_ack(&[7u8; 32], 1).unwrap();
    dave.storage()
        .registry_activation()
        .save_activation(&old_id, &tracker)
        .unwrap();

    let field = ContactField::new(FieldType::Email, "Work", "dave@example.com", 0);
    let field_id = field.id().to_string();
    dave.add_own_field(field).unwrap();
    dave.set_own_field_public(&field_id).unwrap();
    (world, blob)
}

// @internal
#[test]
fn after_the_rekey_the_contact_starts_the_handshake_with_the_new_identity() {
    let (world, blob) = dave_active_with_the_old_identity();
    let dave = world.dave.vauchi();
    assert_eq!(
        dave.queue_registry_pushes().unwrap(),
        0,
        "the old identity confirmed Dave's registry"
    );

    dave.take_rekey_offer(&blob).unwrap();

    assert_eq!(dave.queue_registry_pushes().unwrap(), 1);
}

// @internal
#[test]
fn after_the_rekey_the_contact_sends_nothing_to_the_old_identitys_devices() {
    let (world, blob) = dave_active_with_the_old_identity();
    let dave = world.dave.vauchi();
    let new_id = hex::encode(signing_pk(&world.phone));

    dave.take_rekey_offer(&blob).unwrap();

    let current = dave.storage().contacts().load_own_card().unwrap().unwrap();
    let sent = dave.prepare_card_updates_for_contact(&new_id, &ContactCard::new("Dave"), &current);
    assert!(
        matches!(sent, Err(VauchiError::SendDeferred)),
        "Dave answers the new phone's ratchet, so he waits for it to speak first; got {:?}",
        sent.map(|copies| copies.len())
    );
}
