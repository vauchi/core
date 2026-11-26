// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(feature = "notice-core")]

use vauchi_core::contact::Contact;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::crypto::{SymmetricKey, X25519KeyPair};
use vauchi_core::identity::Identity;
use vauchi_core::notice::pairing::open_directions;
use vauchi_core::storage::{NoticeStateKind, PendingUpdate, UpdateStatus};
use vauchi_core::sync::registry_activation::ActivationTracker;
use vauchi_core::{AuthMode, ContactCard, Vauchi, VauchiConfig};

const PASSWORD: &str = "correct-horse-battery-staple-restore";
const APP_PASSWORD: &str = "normal-notice-pin-1234";
const PIN: &str = "112233";
const PEER: [u8; 32] = [9; 32];
const KINDS: [NoticeStateKind; 4] = [
    NoticeStateKind::Sender,
    NoticeStateKind::Receiver,
    NoticeStateKind::Publisher,
    NoticeStateKind::Pending,
];

struct Exported {
    backup: String,
    real_contact: String,
    mask_contact: String,
    mask_owner: [u8; 32],
    mask_device: [u8; 32],
    mask_exchange: [u8; 32],
    previous_epoch: [u8; 16],
    peer_exchange: [u8; 32],
}

fn seed_states(vauchi: &Vauchi, owner: u8) -> String {
    let contact = Contact::from_exchange(
        *Identity::from_device_link([owner; 32], "Bob".into(), 0, "Phone".into(), 0)
            .signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::from_bytes([7; 32]),
        0,
    );
    vauchi.storage().contacts().save_contact(&contact).unwrap();
    for kind in KINDS {
        vauchi
            .storage()
            .notices()
            .save(contact.id(), kind, &PEER, b"must-not-enter-a-backup")
            .unwrap();
    }
    contact.id().to_owned()
}

fn exported() -> Exported {
    let mut source = Vauchi::in_memory().unwrap();
    source.create_identity("Alice").unwrap();
    source.setup_app_password(APP_PASSWORD).unwrap();
    source.setup_duress_pin_with_biometric_unlock(PIN).unwrap();
    let real_contact = seed_states(&source, 5);
    assert_eq!(source.authenticate(PIN).unwrap(), AuthMode::Duress);
    let mask_contact = seed_states(&source, 6);
    let identity = source.identity().unwrap();
    let mask_owner = *identity.signing_public_key();
    let mask_device = *identity.device_id();
    let mask_exchange = *identity.device_info().exchange_public_key();
    let peer_exchange = *X25519KeyPair::generate().public_key();
    let (sender, _) = open_directions(
        &[7; 32],
        identity.device_info().exchange_keypair(),
        &peer_exchange,
        0,
    );
    let previous_epoch = sender.writes_in();
    // Retain a real registry as well as the profile's device key.
    let registry = source
        .storage()
        .device()
        .own_device_registry(identity)
        .unwrap();
    source
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    seed_old_pair_state(&source, &mask_contact, &peer_exchange);
    assert_eq!(source.authenticate(APP_PASSWORD).unwrap(), AuthMode::Normal);
    Exported {
        backup: source.export_full_backup(PASSWORD).unwrap(),
        real_contact,
        mask_contact,
        mask_owner,
        mask_device,
        mask_exchange,
        previous_epoch,
        peer_exchange,
    }
}

fn restore(backup: &str) -> Vauchi {
    let mut target = Vauchi::in_memory().unwrap();
    target.import_full_backup(backup, PASSWORD).unwrap();
    target
}

fn seed_old_pair_state(source: &Vauchi, contact: &str, peer: &[u8; 32]) {
    let ratchet =
        DoubleRatchetState::initialize_initiator(&SymmetricKey::from_bytes([7; 32]), *peer)
            .unwrap();
    source
        .storage()
        .ratchets()
        .save_ratchet_state_for_device(contact, &PEER, &ratchet, true)
        .unwrap();
    let mut activation = ActivationTracker::new();
    activation.record_push_sent([1; 32], 1);
    activation.record_ack(&[1; 32], 1).unwrap();
    source
        .storage()
        .registry_activation()
        .save_activation(contact, &activation)
        .unwrap();
    source
        .storage()
        .pending()
        .queue_update(&PendingUpdate {
            id: "old-device-queue".into(),
            contact_id: contact.into(),
            update_type: "registry_handshake".into(),
            payload: b"old-device-sealed-bytes".to_vec(),
            created_at: 0,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
            target_device_id: Some(PEER),
        })
        .unwrap();
}

// @internal
#[test]
fn a_restored_mask_bootstraps_without_the_old_devices_pair_sessions() {
    let exported = exported();
    let mut target = restore(&exported.backup);
    assert_eq!(target.authenticate(PIN).unwrap(), AuthMode::Duress);
    assert!(
        target
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&exported.mask_contact, &PEER)
            .unwrap()
            .is_none()
    );
    assert!(
        target
            .storage()
            .registry_activation()
            .load_activation(&exported.mask_contact)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        target
            .storage()
            .pending()
            .get_all_pending_updates()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(target.queue_registry_pushes().unwrap(), 1);
    let pending = target
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_ne!(pending[0].id, "old-device-queue");
    assert_eq!(pending[0].update_type, "registry_handshake");
}

// @internal
#[test]
fn a_restored_mask_keeps_its_new_device_after_a_physical_reopen() {
    let exported = exported();
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let config =
        VauchiConfig::with_storage_path(dir.path().join("restore.db")).with_storage_key(key);
    let mut target = Vauchi::new(config.clone()).unwrap();
    target
        .import_full_backup(&exported.backup, PASSWORD)
        .unwrap();
    let shown = target.public_id().unwrap();
    let fingerprint = target.own_fingerprint().unwrap();
    assert_eq!(target.authenticate(PIN).unwrap(), AuthMode::Duress);
    let device = *target.identity().unwrap().device_id();
    assert_eq!(target.public_id().unwrap(), shown);
    assert_eq!(target.own_fingerprint().unwrap(), fingerprint);
    assert_ne!(device, exported.mask_device);
    drop(target);
    let mut reopened = Vauchi::new(config).unwrap();
    assert_eq!(reopened.authenticate(PIN).unwrap(), AuthMode::Duress);
    assert_eq!(*reopened.identity().unwrap().device_id(), device);
    assert_eq!(reopened.public_id().unwrap(), shown);
    assert_eq!(reopened.own_fingerprint().unwrap(), fingerprint);
}

// @internal
#[test]
fn a_restored_mask_can_be_exported_again_as_a_decoy_payload() {
    const DECOY_PASSWORD: &str = "silver-river-maple-lantern-2026";
    let exported = exported();
    let mut target = restore(&exported.backup);
    assert_eq!(target.authenticate(PIN).unwrap(), AuthMode::Duress);
    let restored_device = *target.identity().unwrap().device_id();
    assert_eq!(target.authenticate(APP_PASSWORD).unwrap(), AuthMode::Normal);
    let backup = target
        .export_full_backup_with_decoy(PASSWORD, DECOY_PASSWORD)
        .unwrap();
    let mut decoy = Vauchi::in_memory().unwrap();
    decoy.import_full_backup(&backup, DECOY_PASSWORD).unwrap();
    let identity = decoy.identity().unwrap();
    assert_eq!(*identity.signing_public_key(), exported.mask_owner);
    assert_ne!(*identity.device_id(), exported.mask_device);
    assert_ne!(*identity.device_id(), restored_device);
    assert!(
        decoy
            .storage()
            .contacts()
            .load_contact(&exported.mask_contact)
            .unwrap()
            .is_some()
    );
}

// @internal
#[test]
fn a_password_backup_restores_no_notice_state_from_either_profile() {
    let exported = exported();
    let mut target = restore(&exported.backup);
    for contact in [&exported.real_contact, &exported.mask_contact] {
        assert!(
            target
                .storage()
                .contacts()
                .load_contact(contact)
                .unwrap()
                .is_some()
        );
        for kind in KINDS {
            assert_eq!(
                target
                    .storage()
                    .notices()
                    .load(contact, kind, &PEER)
                    .unwrap(),
                None
            );
        }
        if contact == &exported.real_contact {
            assert_eq!(target.authenticate(PIN).unwrap(), AuthMode::Duress);
        }
    }
}

// @internal
#[test]
fn a_restored_mask_keeps_its_identity_but_opens_fresh_device_directions() {
    let exported = exported();
    let mut target = restore(&exported.backup);
    assert_eq!(target.authenticate(PIN).unwrap(), AuthMode::Duress);
    let identity = target.identity().unwrap();
    assert_eq!(*identity.signing_public_key(), exported.mask_owner);
    assert_ne!(*identity.device_id(), exported.mask_device);
    assert_ne!(
        *identity.device_info().exchange_public_key(),
        exported.mask_exchange
    );
    let (sender, _) = open_directions(
        &[7; 32],
        identity.device_info().exchange_keypair(),
        &exported.peer_exchange,
        0,
    );
    assert_ne!(sender.writes_in(), exported.previous_epoch);
    let registry = target
        .storage()
        .device()
        .own_device_registry(identity)
        .unwrap();
    assert!(
        registry
            .find_device(identity.device_id())
            .unwrap()
            .is_active()
    );
    let restored_device = *identity.device_id();
    assert_eq!(target.authenticate(APP_PASSWORD).unwrap(), AuthMode::Normal);
    assert_eq!(target.authenticate(PIN).unwrap(), AuthMode::Duress);
    assert_eq!(*target.identity().unwrap().device_id(), restored_device);
}
