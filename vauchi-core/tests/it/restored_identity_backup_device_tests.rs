// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An identity-only backup restored onto a fresh install joins the identity
//! as a new device, as a full backup restore does since vauchi/private#1247
//! (SC-62, vauchi/private#970 plan step 6). Before, the restore kept the
//! backed-up device index and took a new key, so a contact that pinned the
//! backed-up device refused the restored install's registry for good.
//!
//! These tests show what a contact does with the restored install's
//! registry once it arrives, delivered here over the original session. An
//! identity backup restores no contacts and no sessions; whether the
//! restored install can reach a contact it met before is a separate
//! question (vauchi/private#1285).

use crate::common::device_sync::device_signed_broadcast;
use crate::common::per_device::{Party, two_parties};
use vauchi_core::Vauchi;
use vauchi_core::identity::RegistryBroadcast;

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-restore";

fn restore_identity_backup(p: &Party) -> Vauchi {
    let backup = p.bob.export_backup(BACKUP_PASSWORD).unwrap();
    let mut restored = Vauchi::in_memory().unwrap();
    restored.import_backup(&backup, BACKUP_PASSWORD).unwrap();
    restored
}

fn restored_broadcast(restored: &Vauchi) -> RegistryBroadcast {
    restored
        .own_registry_broadcast(restored.identity().unwrap())
        .unwrap()
}

fn restored_device_key(restored: &Vauchi) -> [u8; 32] {
    *restored
        .storage()
        .device()
        .load_or_create_device_signing_key()
        .unwrap()
        .public_key()
        .as_bytes()
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

// @scenario: backup_format_versioning :: Restore v2 backup with correct password
// @internal
#[test]
fn an_identity_backup_restore_is_a_new_device_of_the_same_identity() {
    let p = two_parties();

    let restored = restore_identity_backup(&p);

    assert_eq!(restored.public_id().unwrap(), p.bob.public_id().unwrap());
    assert_ne!(
        restored.identity().unwrap().device_id(),
        &p.bob_device_id(),
        "the restore must not sign as the backed-up device"
    );
}

// @scenario: backup_format_versioning :: Restore v2 backup with correct password
// @internal
#[test]
fn a_contact_that_pinned_the_backed_up_device_accepts_the_restored_registry() {
    let p = two_parties();
    p.push_to_alice(&device_signed_broadcast(&p.bob)).unwrap();

    let restored = restore_identity_backup(&p);
    let restored_device = *restored.identity().unwrap().device_id();
    let accepted = p.push_to_alice(&restored_broadcast(&restored));

    assert!(accepted.is_ok(), "got {accepted:?}");
    assert!(
        alice_holds_as_active(&p, &restored_device),
        "the contact must hold the restored install as an active device"
    );
    assert_eq!(
        p.alice_pin_for(&restored_device),
        Some(restored_device_key(&restored)),
        "the contact must pin the restored install's own key"
    );
    assert_eq!(
        p.alice_pin_for(&p.bob_device_id()),
        Some(*p.bob_device_key().public_key().as_bytes()),
        "the backed-up device's pin must not change"
    );
}
