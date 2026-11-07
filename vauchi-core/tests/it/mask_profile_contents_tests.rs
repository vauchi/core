// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The mask profile's own card, emergency config and seed live in the sealed
//! mask database, so the mask touches one row of the real file only
//! (ADR-080 step 1, vauchi/private#1117).

use std::path::PathBuf;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::emergency::EmergencyBroadcastConfig;
use vauchi_core::storage::Storage;
use vauchi_core::{SymmetricKey, Vauchi, VauchiConfig};

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

    fn vauchi(&self) -> Vauchi {
        let config = VauchiConfig::with_storage_path(&self.path).with_storage_key(self.key.clone());
        Vauchi::new(config).unwrap()
    }

    fn storage(&self) -> Storage {
        Storage::open(&self.path, self.key.clone()).unwrap()
    }

    fn rows_in_file(&self, table: &str) -> i64 {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}

/// Alice with an app password; no duress PIN.
fn alice_with_an_app_password() -> OnDisk {
    let disk = OnDisk::new();
    let mut vauchi = disk.vauchi();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("my-pin-1234").unwrap();
    disk
}

fn real_alert() -> EmergencyBroadcastConfig {
    EmergencyBroadcastConfig {
        trusted_contact_ids: vec!["bob".into()],
        message: "REAL-DISTRESS-MARKER".into(),
        include_location: true,
    }
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn the_duress_own_card_copy_is_sealed_and_not_a_row_of_the_real_file() {
    let disk = alice_with_an_app_password();

    let copy = disk.storage().duress_own_card().load().unwrap();

    assert_eq!(
        copy.map(|c| c.display_name().to_string()),
        Some("Alice".into())
    );
    assert_eq!(disk.rows_in_file("duress_own_card"), 0);
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn an_edit_to_the_duress_own_card_survives_a_reopen() {
    let disk = alice_with_an_app_password();
    disk.storage()
        .duress_own_card()
        .save(&ContactCard::new("Cover Name"))
        .unwrap();

    let copy = disk.storage().duress_own_card().load().unwrap();

    assert_eq!(
        copy.map(|c| c.display_name().to_string()),
        Some("Cover Name".into())
    );
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn an_upgraded_install_keeps_the_duress_own_card_it_had_in_the_real_file() {
    let disk = alice_with_an_app_password();
    let legacy = serde_json::to_vec(&ContactCard::new("Old Cover")).unwrap();
    let legacy = vauchi_core::crypto::encrypt(&disk.key, &legacy).unwrap();
    rusqlite::Connection::open(&disk.path)
        .unwrap()
        .execute(
            "INSERT INTO duress_own_card (id, card_json, card_json_encrypted, updated_at)
             VALUES (1, '', ?1, 0)",
            [legacy],
        )
        .unwrap();

    let copy = disk.storage().duress_own_card().load().unwrap();

    assert_eq!(
        copy.map(|c| c.display_name().to_string()),
        Some("Old Cover".into())
    );
    assert_eq!(disk.rows_in_file("duress_own_card"), 0);
}

// @scenario: duress_mode :: Settings reflect decoy state
#[test]
fn the_decoy_emergency_config_starts_not_set_up() {
    let disk = alice_with_an_app_password();
    disk.storage()
        .emergency()
        .save_emergency_config(&real_alert())
        .unwrap();

    let decoy = disk
        .storage()
        .duress_emergency()
        .load_emergency_config()
        .unwrap();

    assert!(decoy.is_none());
}

// @scenario: duress_mode :: Settings reflect decoy state
#[test]
fn a_decoy_emergency_config_is_sealed_and_leaves_the_real_one_alone() {
    let disk = alice_with_an_app_password();
    let decoy = EmergencyBroadcastConfig {
        trusted_contact_ids: vec!["decoy-dana".into()],
        message: "decoy alert".into(),
        include_location: false,
    };
    disk.storage()
        .duress_emergency()
        .save_emergency_config(&decoy)
        .unwrap();

    let storage = disk.storage();

    let reloaded = storage.duress_emergency().load_emergency_config().unwrap();
    assert_eq!(reloaded.map(|c| c.message), Some("decoy alert".into()));
    assert!(
        storage
            .emergency()
            .load_emergency_config()
            .unwrap()
            .is_none()
    );
    assert_eq!(disk.rows_in_file("emergency_config"), 0);
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn an_app_password_alone_gives_the_install_a_mask_seed() {
    let disk = OnDisk::new();
    let mut vauchi = disk.vauchi();
    vauchi.create_identity("Alice").unwrap();
    assert!(vauchi.storage().mask_seed().load().unwrap().is_none());

    vauchi.setup_app_password("my-pin-1234").unwrap();
    let seed = vauchi.storage().mask_seed().load().unwrap();
    drop(vauchi);

    assert!(seed.is_some());
    assert_eq!(disk.storage().mask_seed().load().unwrap(), seed);
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn the_mask_seed_outlives_disabling_duress_and_a_new_app_password() {
    let disk = alice_with_an_app_password();
    let seed = disk.storage().mask_seed().load().unwrap();
    let mut vauchi = disk.vauchi();
    vauchi.authenticate("my-pin-1234").unwrap();
    vauchi.setup_duress_password("654321").unwrap();
    vauchi.disable_duress().unwrap();
    vauchi
        .change_app_password("my-pin-1234", "new-pin-5678")
        .unwrap();
    drop(vauchi);

    assert_eq!(disk.storage().mask_seed().load().unwrap(), seed);
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn an_upgraded_install_with_an_app_password_gets_a_mask_seed_on_open() {
    let disk = OnDisk::new();
    let mut vauchi = disk.vauchi();
    vauchi.create_identity("Alice").unwrap();
    drop(vauchi);
    disk.storage().identity().save_identity(b"").unwrap();
    // An app password set before mask seeds existed: the hash is in the file
    // and the sealed blob has no seed.
    let hash = vauchi_core::crypto::encrypt(&disk.key, &[7u8; 32]).unwrap();
    rusqlite::Connection::open(&disk.path)
        .unwrap()
        .execute(
            "UPDATE identity SET password_hash_encrypted = ?1, password_salt = ?2 WHERE id = 1",
            rusqlite::params![hash, [9u8; 16].as_slice()],
        )
        .unwrap();

    let first = disk.storage().mask_seed().load().unwrap();
    let second = disk.storage().mask_seed().load().unwrap();

    assert!(first.is_some());
    assert_eq!(first, second);
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn the_mask_contents_survive_a_storage_rekey() {
    let disk = alice_with_an_app_password();
    let mut storage = disk.storage();
    storage
        .duress_emergency()
        .save_emergency_config(&real_alert())
        .unwrap();
    let seed = storage.mask_seed().load().unwrap();
    let new_key = SymmetricKey::generate();

    storage.rekey(new_key.clone()).unwrap();
    drop(storage);
    let storage = Storage::open(&disk.path, new_key).unwrap();

    assert_eq!(
        storage
            .duress_own_card()
            .load()
            .unwrap()
            .map(|c| c.display_name().to_string()),
        Some("Alice".into())
    );
    assert!(
        storage
            .duress_emergency()
            .load_emergency_config()
            .unwrap()
            .is_some()
    );
    assert_eq!(storage.mask_seed().load().unwrap(), seed);
}
