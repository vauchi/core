// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(feature = "notice-core")]

use vauchi_core::contact::Contact;
use vauchi_core::crypto::{SymmetricKey, X25519KeyPair};
use vauchi_core::notice::pairing::open_directions;
use vauchi_core::storage::NoticeStateKind;
use vauchi_core::{AuthMode, ContactCard, Vauchi};

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
        [owner; 32],
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
