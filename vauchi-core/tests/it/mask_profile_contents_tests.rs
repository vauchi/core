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
use vauchi_core::storage::migration::{MigrationRunner, all_migrations};
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

    fn file_has_table(&self, table: &str) -> bool {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap()
    }
}

/// The duress own card as v87 to v113 kept it: a row of the file, under the
/// SEK.
const LEGACY_DURESS_OWN_CARD: &str = "
    CREATE TABLE IF NOT EXISTS duress_own_card (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        card_json TEXT NOT NULL,
        card_json_encrypted BLOB,
        updated_at INTEGER NOT NULL
    );";

fn insert_legacy_duress_own_card(conn: &rusqlite::Connection, key: &SymmetricKey, name: &str) {
    let card = serde_json::to_vec(&ContactCard::new(name)).unwrap();
    conn.execute(
        "INSERT INTO duress_own_card (id, card_json, card_json_encrypted, updated_at)
         VALUES (1, '', ?1, 0)",
        [vauchi_core::crypto::encrypt(key, &card).unwrap()],
    )
    .unwrap();
}

fn migrate_to(conn: &rusqlite::Connection, key: &SymmetricKey, version: u32) {
    let subset: Vec<_> = all_migrations()
        .iter()
        .filter(|m| m.version <= version)
        .copied()
        .collect();
    MigrationRunner::run(conn, key, &subset, None, 0).unwrap();
}

fn duress_own_card_name(storage: &Storage) -> Option<String> {
    storage
        .duress_own_card()
        .load()
        .unwrap()
        .map(|c| c.display_name().to_string())
}

/// Alice with an app password; no duress PIN.
fn alice_with_an_app_password() -> OnDisk {
    let disk = OnDisk::new();
    let mut vauchi = disk.vauchi();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("my-pin-1234").unwrap();
    disk
}

// @internal
#[test]
fn an_exchanged_mask_contact_and_ratchet_survive_reopening_without_another_write() {
    let disk = alice_with_an_app_password();
    let mut vauchi = disk.vauchi();
    vauchi.authenticate("my-pin-1234").unwrap();
    vauchi
        .setup_duress_pin_with_biometric_unlock("654321")
        .unwrap();
    assert_eq!(
        vauchi.authenticate("654321").unwrap(),
        vauchi_core::api::AuthMode::Duress
    );
    let earlier = vauchi_core::Identity::create("Earlier contact", 0);
    vauchi
        .add_contact(vauchi_core::Contact::from_exchange(
            *earlier.signing_public_key(),
            ContactCard::new("Earlier contact"),
            SymmetricKey::generate(),
            0,
        ))
        .unwrap();
    let bob = vauchi_core::Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let expected = *shared.as_bytes();
    let (ratchet, _) = crate::common::helpers::setup_ratchets(&shared);
    let contact = vauchi_core::Contact::from_exchange(
        *bob.signing_public_key(),
        ContactCard::new("Bob"),
        shared,
        0,
    );
    let contact_id = contact.id().to_owned();

    vauchi
        .save_exchanged_contact(&contact, &ratchet, true)
        .unwrap();
    drop(vauchi);

    let mut reopened = disk.vauchi();
    assert_eq!(
        reopened.authenticate("654321").unwrap(),
        vauchi_core::api::AuthMode::Duress
    );
    let persisted = reopened
        .get_contact(&contact_id)
        .unwrap()
        .expect("exchange was sealed before success");
    assert_eq!(persisted.display_name(), "Bob");
    assert_eq!(persisted.shared_key().unwrap().as_bytes(), &expected);
    let (stored_ratchet, initiator) = reopened
        .storage()
        .ratchets()
        .load_ratchet_state(&contact_id)
        .unwrap()
        .expect("exchange ratchet was sealed");
    assert!(initiator);
    assert_eq!(
        serde_json::to_vec(&stored_ratchet.serialize()).unwrap(),
        serde_json::to_vec(&ratchet.serialize()).unwrap()
    );
}

// @internal
#[test]
fn an_orphaned_mask_ratchet_is_rejected_without_rewriting_the_sealed_source() {
    let disk = alice_with_an_app_password();
    let mask_key = crate::common::helpers::duress_mask_key(&disk.key);
    let mask = Storage::open_mask_profile(&disk.path, mask_key.clone()).unwrap();
    let bob = vauchi_core::Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let contact = vauchi_core::Contact::from_exchange(
        *bob.signing_public_key(),
        ContactCard::new("Bob"),
        shared.clone(),
        0,
    );
    let contact_id = contact.id().to_owned();
    let (ratchet, _) = crate::common::helpers::setup_ratchets(&shared);
    mask.contacts().save_contact(&contact).unwrap();
    mask.ratchets()
        .save_ratchet_state(&contact_id, &ratchet, true)
        .unwrap();
    let sealed = || -> Vec<u8> {
        rusqlite::Connection::open(&disk.path)
            .unwrap()
            .query_row(
                "SELECT content_encrypted FROM duress_content WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap()
    };
    let valid_seal = sealed();
    // Corrupt only this test-owned source; the production loader keeps FK validation enabled.
    mask.connection()
        .execute_batch("PRAGMA foreign_keys = OFF; DELETE FROM contacts; PRAGMA foreign_keys = ON;")
        .unwrap();
    let probe = vauchi_core::Identity::create("Probe", 0);
    mask.contacts()
        .save_contact(&vauchi_core::Contact::from_exchange(
            *probe.signing_public_key(),
            ContactCard::new("Probe"),
            SymmetricKey::generate(),
            0,
        ))
        .unwrap();
    let invalid_seal = sealed();
    assert!(invalid_seal != valid_seal);
    drop(mask);

    let Err(error) = Storage::open_mask_profile(&disk.path, mask_key.clone()) else {
        panic!("orphaned ratchet must fail foreign-key validation");
    };
    assert!(
        matches!(error, vauchi_core::storage::StorageError::Database(rusqlite::Error::SqliteFailure(error, _)) if error.extended_code == 787)
    );
    assert!(
        sealed() == invalid_seal,
        "failed loading must not rewrite its source"
    );

    rusqlite::Connection::open(&disk.path)
        .unwrap()
        .execute(
            "UPDATE duress_content SET content_encrypted = ?1 WHERE id = 1",
            [valid_seal],
        )
        .unwrap();
    let restored = Storage::open_mask_profile(&disk.path, mask_key).unwrap();
    let restored_contact = restored
        .contacts()
        .load_contact(&contact_id)
        .unwrap()
        .expect("restored parent contact");
    assert_eq!(restored_contact.display_name(), "Bob");
    assert_eq!(
        restored_contact.shared_key().unwrap().as_bytes(),
        shared.as_bytes()
    );
    let (restored_ratchet, initiator) = restored
        .ratchets()
        .load_ratchet_state(&contact_id)
        .unwrap()
        .expect("restored child ratchet");
    assert!(initiator);
    assert_eq!(
        serde_json::to_vec(&restored_ratchet.serialize()).unwrap(),
        serde_json::to_vec(&ratchet.serialize()).unwrap()
    );
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
fn the_duress_own_card_copy_is_sealed_and_the_file_has_no_table_for_it() {
    let disk = alice_with_an_app_password();

    let name = duress_own_card_name(&disk.storage());

    assert_eq!(name, Some("Alice".into()));
    assert!(!disk.file_has_table("duress_own_card"));
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn an_edit_to_the_duress_own_card_survives_a_reopen() {
    let disk = alice_with_an_app_password();
    disk.storage()
        .duress_own_card()
        .save(&ContactCard::new("Cover Name"))
        .unwrap();

    let name = duress_own_card_name(&disk.storage());

    assert_eq!(name, Some("Cover Name".into()));
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn an_install_upgraded_from_before_the_sealed_duress_content_keeps_its_duress_own_card() {
    let disk = OnDisk::new();
    let conn = rusqlite::Connection::open(&disk.path).unwrap();
    migrate_to(&conn, &disk.key, 93);
    insert_legacy_duress_own_card(&conn, &disk.key, "Old Cover");
    drop(conn);

    let name = duress_own_card_name(&disk.storage());

    assert_eq!(name, Some("Old Cover".into()));
    assert!(!disk.file_has_table("duress_own_card"));
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn an_install_upgraded_with_its_duress_content_sealed_keeps_its_duress_own_card() {
    let disk = alice_with_an_app_password();
    let conn = rusqlite::Connection::open(&disk.path).unwrap();
    conn.execute_batch(LEGACY_DURESS_OWN_CARD).unwrap();
    insert_legacy_duress_own_card(&conn, &disk.key, "Old Cover");
    conn.execute("DELETE FROM schema_version WHERE version >= 114", [])
        .unwrap();
    drop(conn);

    let name = duress_own_card_name(&disk.storage());

    assert_eq!(name, Some("Old Cover".into()));
    assert!(!disk.file_has_table("duress_own_card"));
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
    let seed = vauchi
        .storage()
        .mask_seed()
        .load()
        .unwrap()
        .expect("app password creates its mask seed");
    drop(vauchi);

    assert_eq!(disk.storage().mask_seed().load().unwrap(), Some(seed));
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn the_mask_seed_outlives_disabling_duress_and_a_new_app_password() {
    let disk = alice_with_an_app_password();
    let seed = disk.storage().mask_seed().load().unwrap();
    let mut vauchi = disk.vauchi();
    vauchi.authenticate("my-pin-1234").unwrap();
    vauchi
        .setup_duress_pin_with_biometric_unlock("654321")
        .unwrap();
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

    let first = first.expect("upgraded install creates its mask seed");
    assert_eq!(Some(first), second);
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

    assert_eq!(duress_own_card_name(&storage), Some("Alice".into()));
    assert!(
        storage
            .duress_emergency()
            .load_emergency_config()
            .unwrap()
            .is_some()
    );
    assert_eq!(storage.mask_seed().load().unwrap(), seed);
}
