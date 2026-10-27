// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shredding my identity: a panic shred tells every contact and the relay
//! before it is gone, and any shred leaves nothing on the device.

use base64::Engine;
use vauchi_core::api::{
    DeletionManager, PreSignedPurgeRequest, PurgeSender, RevocationSender, ShredError,
    ShredManager, ShredToken,
};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{ShreddingMasterKey, SymmetricKey};
use vauchi_core::identity::Identity;
use vauchi_core::network::revocation::decode_revocation_blob;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage, Storage};

const SMK_KEY_NAME: &str = "smk";

#[derive(Default)]
struct Relay {
    purged: bool,
}

impl PurgeSender for Relay {
    fn send_purge(&mut self, _: &PreSignedPurgeRequest, _: u64) -> Result<bool, ShredError> {
        self.purged = true;
        Ok(true)
    }
}

#[derive(Default)]
struct Mailboxes {
    blobs: Vec<String>,
}

impl RevocationSender for Mailboxes {
    fn send_revocation_delivery(
        &mut self,
        _token: &str,
        blob_b64: &str,
        _: u64,
    ) -> Result<bool, ShredError> {
        self.blobs.push(blob_b64.to_string());
        Ok(true)
    }
}

fn exchanged(storage: &Storage, name: &str) -> String {
    let peer = Identity::create(name, 0);
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    id
}

// @scenario: privacy_compliance :: Panic shred immediately destroys all data and notifies contacts
#[test]
fn a_panic_shred_revokes_each_contact_purges_the_relay_and_destroys_everything() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(&dir.path().join("vauchi.db"), SymmetricKey::generate()).unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key(
        SMK_KEY_NAME,
        ShreddingMasterKey::derive_from_seed(&[0x42; 32]).as_bytes(),
    )
    .unwrap();
    let me = Identity::create("Me", 0);
    let mut contacts = vec![exchanged(&storage, "Alice"), exchanged(&storage, "Bob")];
    let shred = ShredManager::new(&storage, &keys, &me, dir.path());
    let (mut relay, mut mailboxes) = (Relay::default(), Mailboxes::default());

    let report = shred
        .panic_shred(Some(&mut relay), Some(&mut mailboxes))
        .unwrap();

    let mut revoked: Vec<String> = mailboxes
        .blobs
        .iter()
        .map(|b64| {
            let blob = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .unwrap();
            let revocation = decode_revocation_blob(&blob).expect("a revocation");
            assert!(revocation.verify(me.signing_public_key()), "signed by me");
            revocation.recipient_id.to_string()
        })
        .collect();
    revoked.sort();
    contacts.sort();
    assert_eq!(revoked, contacts);
    assert_eq!(report.contacts_notified, 2);
    assert!(relay.purged && report.relay_purge_sent);
    assert!(keys.load_key(SMK_KEY_NAME).unwrap().is_none());
    assert!(report.smk_destroyed && report.sqlite_destroyed);
}

// @scenario: privacy_compliance :: Deletion is thorough
#[test]
fn deleting_my_identity_leaves_no_key_database_or_cached_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("vauchi");
    std::fs::create_dir_all(data_dir.join("avatars")).unwrap();
    std::fs::write(data_dir.join("avatars").join("bob.webp"), b"cached").unwrap();
    let storage = Storage::open(&data_dir.join("vauchi.db"), SymmetricKey::generate()).unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key(
        SMK_KEY_NAME,
        ShreddingMasterKey::derive_from_seed(&[0x42; 32]).as_bytes(),
    )
    .unwrap();
    keys.save_key("storage_bootstrap", &[7; 32]).unwrap();
    let me = Identity::create("Me", 0);
    exchanged(&storage, "Bob");
    DeletionManager::new(&storage)
        .schedule_deletion_with_execute_at(1000, 999)
        .unwrap();
    let shred = ShredManager::new(&storage, &keys, &me, &data_dir);

    shred
        .hard_shred(ShredToken::from_created_at(1000), None, None)
        .unwrap();

    let left = shred.verify_shred();
    assert_eq!(
        (
            left.smk_absent,
            left.bootstrap_key_absent,
            left.database_absent,
            left.pre_signed_absent,
            left.data_dir_absent,
        ),
        (true, true, true, true, true)
    );
    assert!(!data_dir.join("avatars").join("bob.webp").exists());
}
