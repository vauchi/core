// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::card::ContactCard;
use crate::contact::Contact;
use crate::storage::NoticeStateKind;

const PEER: [u8; 32] = [9; 32];

fn seeded() -> (Storage, String) {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = seed_mask(&storage);
    (storage, contact)
}

fn seed_mask(storage: &Storage) -> String {
    storage.mask_seed().ensure().unwrap();
    let contact = Contact::from_exchange(
        [6; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let mask = storage.mask_profile();
    mask.contacts().save_contact(&contact).unwrap();
    mask.notices()
        .save(contact.id(), NoticeStateKind::Sender, &PEER, b"old-chain")
        .unwrap();
    contact.id().into()
}

// @internal
#[test]
fn a_failed_outer_backup_commit_restores_the_embedded_mask_in_memory_and_on_disk() {
    use crate::{Vauchi, VauchiConfig};

    const PASSWORD: &str = "correct-horse-battery-staple-restore";
    let mut source = Vauchi::in_memory().unwrap();
    source.create_identity("Alice").unwrap();
    source.setup_app_password("normal-pin-1234").unwrap();
    source
        .setup_duress_pin_with_biometric_unlock("112233")
        .unwrap();
    seed_mask(source.storage());
    let backup = source.export_full_backup(PASSWORD).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("restore.db"))
        .with_storage_key(SymmetricKey::generate());
    let mut target = Vauchi::new(config.clone()).unwrap();
    let contact = seed_mask(target.storage());
    let previous = target
        .storage()
        .mask_seed()
        .identity("Mask", 0)
        .unwrap()
        .unwrap();
    let mask = target.storage().mask_profile();
    mask.identity()
        .save_identity(&previous.to_storage_bytes())
        .unwrap();
    let registry = mask.device().own_device_registry(&previous).unwrap();
    mask.device().save_device_registry(&registry).unwrap();
    target.storage().seal_mask().unwrap();
    let seed = target.storage().mask_seed().load().unwrap();
    let assert_previous = |install: &Vauchi| {
        assert!(install.identity().is_none());
        assert_eq!(install.storage().contacts().count_contacts().unwrap(), 0);
        assert_eq!(install.storage().mask_seed().load().unwrap(), seed);
        let identity = install
            .storage()
            .mask_seed()
            .identity("Mask", 0)
            .unwrap()
            .unwrap();
        assert_eq!(identity.device_id(), previous.device_id());
        let mask = install.storage().mask_profile();
        assert_eq!(
            mask.device()
                .own_device_registry(&identity)
                .unwrap()
                .to_json(),
            registry.to_json()
        );
        assert_eq!(
            mask.notices()
                .load(&contact, NoticeStateKind::Sender, &PEER)
                .unwrap()
                .unwrap()
                .as_slice(),
            b"old-chain"
        );
    };
    target.storage().arm_commit_fault();
    assert!(target.import_full_backup(&backup, PASSWORD).is_err());
    assert_previous(&target);
    drop(target);
    let mut reopened = Vauchi::new(config).unwrap();
    assert_previous(&reopened);
    reopened.import_full_backup(&backup, PASSWORD).unwrap();
    assert_eq!(reopened.identity().unwrap().display_name(), "Alice");
}

fn assert_no_chain(storage: &Storage, contact: &str) {
    assert!(storage.contacts().load_contact(contact).unwrap().is_some());
    assert_eq!(
        storage
            .notices()
            .load(contact, NoticeStateKind::Sender, &PEER)
            .unwrap(),
        None
    );
}

// @internal
#[test]
fn a_mask_transfer_excludes_notice_keys_before_backup_encryption() {
    let (storage, contact) = seeded();
    let transfer = storage.mask_for_backup().unwrap();
    let copied = Storage::empty_mask_profile(transfer.key.clone(), None).unwrap();
    copied.load_body(&transfer.body, &transfer.key).unwrap();
    assert_no_chain(&copied, &contact);
    assert_eq!(
        storage
            .mask_profile()
            .notices()
            .load(&contact, NoticeStateKind::Sender, &PEER)
            .unwrap()
            .unwrap()
            .as_slice(),
        b"old-chain"
    );
}

// @internal
#[test]
fn restoring_a_legacy_mask_transfer_also_discards_its_notice_keys() {
    let (source, contact) = seeded();
    let mask = source.mask_profile();
    let legacy = MaskTransfer {
        key: mask.encryption_key.clone(),
        body: mask.sealed_body().unwrap(),
    };
    let target = Storage::in_memory(SymmetricKey::generate()).unwrap();
    target.restore_mask_from_backup(&legacy).unwrap();
    assert_no_chain(target.mask_profile(), &contact);
    let identity = target.mask_seed().identity("Mask", 0).unwrap().unwrap();
    assert_ne!(identity.device_info().device_index(), 0);
    assert_eq!(
        target.mask_seed().load().unwrap(),
        source.mask_seed().load().unwrap()
    );
}

// @internal
#[test]
fn a_failed_mask_import_rolls_back_the_device_identity_and_sessions_together() {
    let (source, _) = seeded();
    let mask = source.mask_profile();
    let transfer = MaskTransfer {
        key: mask.encryption_key.clone(),
        body: mask.sealed_body().unwrap(),
    };
    let (target, contact) = seeded();
    let identity = target.mask_seed().identity("Mask", 0).unwrap().unwrap();
    let mask = target.mask_profile();
    let registry = mask.device().own_device_registry(&identity).unwrap();
    mask.device().save_device_registry(&registry).unwrap();
    mask.conn
        .execute_batch(
            "CREATE TEMP TRIGGER refuse_restored_identity BEFORE INSERT ON identity
         BEGIN SELECT RAISE(ABORT, 'injected restore failure'); END;",
        )
        .unwrap();
    let refused = target.restore_mask_from_backup(&transfer);
    assert!(matches!(refused, Err(StorageError::Database(_))));
    let after = target.mask_seed().identity("Mask", 0).unwrap().unwrap();
    assert_eq!(after.device_id(), identity.device_id());
    assert_eq!(after.signing_public_key(), identity.signing_public_key());
    assert_eq!(
        mask.device().own_device_registry(&after).unwrap().to_json(),
        registry.to_json()
    );
    assert_eq!(
        mask.notices()
            .load(&contact, NoticeStateKind::Sender, &PEER)
            .unwrap()
            .unwrap()
            .as_slice(),
        b"old-chain"
    );
}

// @internal
#[test]
fn a_mask_refuses_a_saved_device_identity_from_another_seed() {
    let (storage, _) = seeded();
    let other = crate::identity::Identity::create("Other", 0);
    storage
        .mask_profile()
        .identity()
        .save_identity(&other.to_storage_bytes())
        .unwrap();
    let refused = storage.mask_seed().identity("Mask", 0);
    assert!(matches!(refused, Err(StorageError::InvalidData(_))));
}
