// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The mask profile's database has the regular profile schema and opens as
//! an ordinary `Storage` under the mask key, in memory, reaching the real
//! file only through its sealed row (ADR-080 point 4, amended 2026-11-08;
//! vauchi/private#1117).

use std::path::PathBuf;

use vauchi_core::SymmetricKey;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::storage::Storage;

use crate::common::helpers::duress_mask_key;

struct OnDisk {
    _dir: tempfile::TempDir,
    path: PathBuf,
    key: SymmetricKey,
}

impl OnDisk {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        OnDisk {
            path: dir.path().join("vauchi.db"),
            key: SymmetricKey::generate(),
            _dir: dir,
        }
    }

    fn real(&self) -> Storage {
        Storage::open(&self.path, self.key.clone()).unwrap()
    }

    fn mask(&self) -> Storage {
        Storage::open_mask_profile(&self.path, duress_mask_key(&self.key)).unwrap()
    }
}

/// The contact id the duress session has always shown for a decoy id.
fn decoy_contact_id(decoy_id: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(decoy_id.as_bytes()))
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn a_decoy_is_a_contact_of_the_mask_profile_and_a_real_contact_is_not() {
    let disk = OnDisk::new();
    let real = disk.real();
    real.decoy()
        .save_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
        .unwrap();
    let bob = Contact::from_exchange(
        [7u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    real.contacts().save_contact(&bob).unwrap();
    drop(real);

    let contacts = disk.mask().contacts().list_contacts().unwrap();

    assert_eq!(
        contacts
            .iter()
            .map(|c| (c.id().to_string(), c.display_name().to_string()))
            .collect::<Vec<_>>(),
        vec![(decoy_contact_id("decoy-dana"), "Decoy Dana".to_string())]
    );
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn a_write_through_the_mask_profile_is_sealed_before_it_returns() {
    let disk = OnDisk::new();
    drop(disk.real());

    disk.mask().tags().create_tag("Book club").unwrap();

    let tags = disk.real().duress_tags().list_tags().unwrap();
    assert_eq!(
        tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Book club"]
    );
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn the_mask_profile_does_not_open_under_the_storage_key() {
    let disk = OnDisk::new();
    drop(disk.real());

    let opened = Storage::open_mask_profile(&disk.path, disk.key.clone());

    assert!(matches!(
        opened,
        Err(vauchi_core::storage::StorageError::Encryption(_))
    ));
}
