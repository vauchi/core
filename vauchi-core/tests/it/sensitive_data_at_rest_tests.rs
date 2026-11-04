// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! adr-constraints "Always encrypt sensitive fields at rest" (ADR-015):
//! after a client stores its own card, a contact's card, the shared key
//! and a ratchet, no file in its storage directory holds any of them in
//! plaintext (private#972, SC-64). Per-table tests cover the columns one
//! by one; this one reads the bytes on disk.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::Identity;
use vauchi_core::{SymmetricKey, Vauchi, VauchiConfig};

const OWN_NAME: &str = "Ottoline Wexford-Brigg";
const OWN_EMAIL: &str = "ottoline.7f3a@example.invalid";
const CONTACT_NAME: &str = "Zebulon Quixote-Ferrante";
const CONTACT_PHONE: &str = "+41 79 555 01 23";

fn files_under(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .map(|path| (path.display().to_string(), std::fs::read(&path).unwrap()))
        .collect()
}

fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

// @internal
#[test]
fn no_card_name_or_key_is_stored_in_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    let shared_secret = SymmetricKey::generate();
    {
        let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
            .with_storage_key(SymmetricKey::generate());
        let mut vauchi = Vauchi::new(config).unwrap();
        vauchi.create_identity(OWN_NAME).unwrap();
        vauchi
            .add_own_field(ContactField::new(FieldType::Email, "Email", OWN_EMAIL, 0))
            .unwrap();

        let mut card = ContactCard::new(CONTACT_NAME);
        card.add_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            CONTACT_PHONE,
            0,
        ))
        .unwrap();
        let peer = Identity::create("peer", 0);
        let contact =
            Contact::from_exchange(*peer.signing_public_key(), card, shared_secret.clone(), 0);
        let contact_id = contact.id().to_string();
        vauchi.add_contact(contact).unwrap();
        let ratchet =
            DoubleRatchetState::initialize_responder(&shared_secret, X3DHKeyPair::generate());
        vauchi
            .storage()
            .ratchets()
            .save_ratchet_state(&contact_id, &ratchet, false)
            .unwrap();
    }

    let files = files_under(dir.path());
    assert!(
        files.iter().any(|(_, bytes)| holds(bytes, b"CREATE TABLE")),
        "the scan must read the raw database, which holds its schema in plaintext"
    );
    let key = shared_secret.as_bytes();
    let secrets: [(&str, Vec<u8>); 6] = [
        ("own name", OWN_NAME.into()),
        ("own email", OWN_EMAIL.into()),
        ("contact name", CONTACT_NAME.into()),
        ("contact phone", CONTACT_PHONE.into()),
        ("shared key", key.to_vec()),
        ("shared key (hex)", hex::encode(key).into_bytes()),
    ];
    for (path, bytes) in &files {
        for (what, secret) in &secrets {
            assert!(
                !holds(bytes, secret),
                "{path} stores the {what} in plaintext"
            );
        }
    }
}
