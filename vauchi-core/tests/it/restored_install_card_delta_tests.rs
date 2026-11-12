// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A full backup restored onto a fresh install registers the install as a
//! new device of the same identity, so the contacts that pinned the backed-up
//! device keep applying its card deltas (vauchi/private#1247; SC-62,
//! vauchi/private#970). Before, the restore signed as the backed-up device
//! with a new key, and every contact refused its deltas for good. Found by
//! e2e's `integration_full_backup_restore_preserves_sharing_and_two_way_updates`
//! on the Core 0.104.0 bump.
//!
//! A delta here is signed as the restored install signs and sealed over the
//! session the original install shared, so only the signing device differs
//! from a delivery before the restore.

use crate::common::card_update::{recipient_pk, seal_delta};
use crate::common::device_sync::device_signed_broadcast;
use crate::common::per_device::{Party, tablet_entry, two_parties};
use vauchi_core::Vauchi;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::RegistryBroadcast;
use vauchi_core::sync::delta::CardDelta;

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-restore";

fn restore(backup: &str) -> Vauchi {
    let mut restored = Vauchi::in_memory().unwrap();
    restored
        .import_full_backup(backup, BACKUP_PASSWORD)
        .unwrap();
    restored
}

fn restored_broadcast(restored: &Vauchi) -> RegistryBroadcast {
    restored
        .own_registry_broadcast(restored.identity().unwrap())
        .unwrap()
}

/// Bob's phone saves its registry, as a phone that ever broadcast it does.
fn bob_saves_his_registry(p: &Party) {
    p.bob
        .storage()
        .device()
        .save_device_registry(&p.bob_registry())
        .unwrap();
}

fn delta_from(restored: &Vauchi, p: &Party) -> Vec<u8> {
    let mut delta = CardDelta::compute(
        &ContactCard::new("Bob"),
        &ContactCard::new("Robert"),
        p.now(),
    );
    delta.set_version(1);
    delta.sign(
        restored.identity().unwrap(),
        &recipient_pk(restored, &p.alice_at_bob),
    );
    let key = restored
        .storage()
        .device()
        .load_or_create_device_signing_key()
        .unwrap();
    delta.sign_by_device(*restored.identity().unwrap().device_id(), &key);
    seal_delta(&p.bob, &p.alice_at_bob, &delta)
}

fn alice_holds_as_active(p: &Party, device_id: &[u8; 32]) -> bool {
    p.alice
        .storage()
        .device()
        .load_contact_active_devices(&p.bob_at_alice)
        .unwrap()
        .iter()
        .any(|device| &device.device_id == device_id)
}

fn bobs_name_at_alice(p: &Party) -> String {
    p.alice
        .get_contact(&p.bob_at_alice)
        .unwrap()
        .unwrap()
        .display_name()
        .to_string()
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
// @internal
#[test]
fn a_restored_install_is_a_new_device_of_the_same_identity() {
    let p = two_parties();
    let backup = p.bob.export_full_backup(BACKUP_PASSWORD).unwrap();

    let restored = restore(&backup);

    assert_eq!(restored.public_id().unwrap(), p.bob.public_id().unwrap());
    assert_ne!(
        restored.identity().unwrap().device_id(),
        &p.bob_device_id(),
        "the restore must not sign as the backed-up device"
    );
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
// @internal
#[test]
fn a_contact_applies_the_restored_install_delta_once_its_registry_arrives() {
    let p = two_parties();
    p.push_to_alice(&device_signed_broadcast(&p.bob)).unwrap();
    let backup = p.bob.export_full_backup(BACKUP_PASSWORD).unwrap();
    let restored = restore(&backup);

    let accepted = p.push_to_alice(&restored_broadcast(&restored));
    let outcome = process_single_card_update(
        p.alice.identity().unwrap(),
        p.alice.storage(),
        &p.bob_at_alice,
        &delta_from(&restored, &p),
    );

    assert!(accepted.is_ok(), "got {accepted:?}");
    assert!(alice_holds_as_active(
        &p,
        restored.identity().unwrap().device_id()
    ));
    assert!(
        matches!(outcome, Ok(ReceiveOutcome::CardDelta)),
        "the restored install's delta must apply, got {outcome:?}"
    );
    assert_eq!(bobs_name_at_alice(&p), "Robert");
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
// @internal
#[test]
fn a_backup_carries_the_registry_and_the_backed_up_device_stays_active() {
    let p = two_parties();
    bob_saves_his_registry(&p);
    let backup = p.bob.export_full_backup(BACKUP_PASSWORD).unwrap();

    let broadcast = restored_broadcast(&restore(&backup));

    assert!(broadcast.contains_device(&p.bob_device_id()));
    assert_eq!(broadcast.active_device_count(), 2);
    assert!(broadcast.version() > p.bob_registry().version());
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
// @internal
#[test]
fn a_contact_accepts_the_restored_registry_after_a_newer_one_than_the_backup() {
    let p = two_parties();
    bob_saves_his_registry(&p);
    let backup = p.bob.export_full_backup(BACKUP_PASSWORD).unwrap();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&SigningKeyPair::generate()), identity_key)
        .unwrap();
    let with_tablet = RegistryBroadcast::signed_by_device(
        &registry,
        identity_key,
        p.bob_device_id(),
        &p.bob_device_key(),
        p.now(),
    );
    p.push_to_alice(&with_tablet).unwrap();

    let restored = restore(&backup);
    let restored_device = *restored.identity().unwrap().device_id();
    assert_ne!(restored_device, p.bob_device_id());
    p.push_to_alice(&restored_broadcast(&restored)).unwrap();

    assert!(
        alice_holds_as_active(&p, &restored_device),
        "a registry newer than the backup must not shadow the restore"
    );
}
