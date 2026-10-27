// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A panic shred tells every contact and the relay before it is gone: no
//! grace period, signed revocations, a purge request, then nothing left.

use base64::Engine;
use vauchi_core::api::{
    PreSignedPurgeRequest, PurgeSender, RevocationSender, ShredError, ShredManager,
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
