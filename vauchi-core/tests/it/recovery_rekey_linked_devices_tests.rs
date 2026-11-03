// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The recovered owner's linked devices share the rekey (vauchi/private#796,
//! D26): a device linked while contacts still wait gets the waiting list,
//! each offer's exchange key, the proof and the old seed, so it can offer,
//! complete and sign continuations as the phone does; a rekey one device
//! completes reaches the others with the new shared key.

use std::sync::Arc;
use std::time::SystemTime;

use vauchi_core::api::RekeyAnswerTaken;
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::clock::FakeClock;
use vauchi_core::identity::DeviceRegistry;
use vauchi_core::sync::device_sync::DeviceLinkIntent;
use vauchi_core::{Identity, Vauchi};

use crate::common::device_sync::{journal_for_tablet, link_tablet};
use crate::common::recovered_phone::{World, recovered};

/// Alice's restored phone, its offers made, before anyone answered.
fn restored_and_offered() -> World {
    let clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut phone = Vauchi::in_memory_with_clock(clock).unwrap();
    phone.create_identity("Alice, new phone").unwrap();
    let world = recovered(phone, None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    world.phone.rekey_offer_deliveries().unwrap();
    world
}

/// A tablet of the phone's identity, linked by the device-link full sync.
fn linked_tablet(phone: &Vauchi) -> Vauchi {
    let identity = phone.identity().unwrap();
    let seed = *identity.master_seed();
    let now = phone.storage().clock().unix_seconds();
    let mut tablet = Vauchi::in_memory().unwrap();
    tablet
        .set_identity(Identity::from_device_link(
            seed,
            "Alice".into(),
            1,
            "Alice tablet".into(),
            now,
        ))
        .unwrap();
    let phone_device = identity.create_device_info(now);
    let registry = DeviceRegistry::new(
        phone_device.to_registered(&seed),
        identity.signing_keypair(),
    );
    let payload = DeviceSyncOrchestrator::new(phone.storage(), phone_device, registry.clone())
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();
    let tablet_device = tablet.identity().unwrap().create_device_info(now);
    DeviceSyncOrchestrator::new(tablet.storage(), tablet_device, registry)
        .apply_full_sync(payload)
        .unwrap();
    tablet
}

fn offer_blobs(device: &Vauchi) -> Vec<(String, Vec<u8>)> {
    device
        .rekey_offer_deliveries()
        .unwrap()
        .into_iter()
        .map(|delivery| (delivery.contact_id, delivery.blob))
        .collect()
}

// @internal
#[test]
fn a_device_linked_while_contacts_wait_makes_the_phones_offers() {
    let world = restored_and_offered();

    let tablet = linked_tablet(&world.phone);

    assert_eq!(
        tablet.rekey_waiting().unwrap(),
        world.phone.rekey_waiting().unwrap()
    );
    assert_eq!(offer_blobs(&tablet), offer_blobs(&world.phone));
}

// @internal
#[test]
fn a_device_linked_while_contacts_wait_holds_the_old_seed() {
    let world = restored_and_offered();

    let tablet = linked_tablet(&world.phone);

    let at_phone = world
        .phone
        .storage()
        .recovery()
        .load_old_identity_seed()
        .unwrap()
        .unwrap();
    let at_tablet = tablet
        .storage()
        .recovery()
        .load_old_identity_seed()
        .unwrap()
        .expect("the tablet signs continuations too");
    assert_eq!(*at_tablet, *at_phone);
}

// @internal
#[test]
fn the_linked_device_completes_with_an_answer_the_phone_did_not_fetch() {
    let world = restored_and_offered();
    let tablet = linked_tablet(&world.phone);
    let offer = offer_blobs(&world.phone).remove(0).1;
    let dave = world.dave.vauchi();
    dave.take_rekey_offer(&offer).unwrap();
    let answer = dave.rekey_answer_deliveries().unwrap()[0].blob.clone();

    let taken = tablet.take_rekey_answer(&answer).unwrap();

    assert_eq!(
        taken,
        RekeyAnswerTaken::Completed {
            contact_id: world.dave_id.clone()
        }
    );
}

// @internal
#[test]
fn a_rekey_one_device_completes_reaches_the_others_with_the_new_shared_key() {
    let world = restored_and_offered();
    let tablet = linked_tablet(&world.phone);
    let (registry, tablet_id) = link_tablet(&world.phone, [71u8; 32]);
    let offer = offer_blobs(&world.phone).remove(0).1;
    let dave = world.dave.vauchi();
    dave.take_rekey_offer(&offer).unwrap();
    let answer = dave.rekey_answer_deliveries().unwrap()[0].blob.clone();
    world.phone.take_rekey_answer(&answer).unwrap();
    let journal = journal_for_tablet(&world.phone, registry, &tablet_id);

    tablet.apply_sync_items(journal).unwrap();

    let at_phone = world.phone.get_contact(&world.dave_id).unwrap().unwrap();
    let at_tablet = tablet.get_contact(&world.dave_id).unwrap().unwrap();
    assert_eq!(
        at_tablet.shared_key().unwrap().as_bytes(),
        at_phone.shared_key().unwrap().as_bytes()
    );
    assert!(
        !tablet
            .rekey_pending_contacts()
            .unwrap()
            .contains(&world.dave_id)
    );
    assert!(
        tablet.rekey_offer_deliveries().unwrap().is_empty(),
        "no more offers to a contact already moved"
    );
}
