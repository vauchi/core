// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::card::ContactCard;
use crate::contact::Contact;
use crate::storage::NoticeStateKind;

const PEER: [u8; 32] = [9; 32];

fn seeded() -> (Storage, String) {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
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
    (storage, contact.id().into())
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
}
