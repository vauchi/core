// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! With secure storage, every key that can open the database lives in it,
//! so deleting the keys there destroys access (ADR-033, vauchi/private#580).
//! Before an identity exists the database is under a bootstrap key; after,
//! under the SMK-derived key. Boot opens with whichever key the data is
//! actually under and finishes an interrupted move to the SMK.

use std::path::Path;
use std::sync::Arc;

use vauchi_core::api::{Vauchi, VauchiConfig, VauchiError};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{ShreddingMasterKey, SymmetricKey};
use vauchi_core::identity::Identity;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage, Storage, StorageError};

const SMK_KEY_NAME: &str = "smk";
const BOOTSTRAP_KEY_NAME: &str = "storage_bootstrap";

fn boot(path: &Path, secure: &Arc<MemoryKeyStorage>) -> Result<Vauchi, VauchiError> {
    Vauchi::with_secure_storage(VauchiConfig::with_storage_path(path), secure.clone())
}

fn boot_with_shell_key(
    path: &Path,
    secure: &Arc<MemoryKeyStorage>,
    shell_key: SymmetricKey,
) -> Result<Vauchi, VauchiError> {
    Vauchi::with_secure_storage(
        VauchiConfig::with_storage_path(path).with_storage_key(shell_key),
        secure.clone(),
    )
}

fn sek_of(secure: &MemoryKeyStorage) -> SymmetricKey {
    let smk: [u8; 32] = secure
        .load_key(SMK_KEY_NAME)
        .unwrap()
        .expect("SMK stored")
        .try_into()
        .unwrap();
    ShreddingMasterKey::from_bytes(smk).derive_sek()
}

fn own_card_name(vauchi: &Vauchi) -> Option<String> {
    vauchi
        .own_card()
        .unwrap()
        .map(|card| card.display_name().to_string())
}

fn assert_wrong_key(result: Result<Vauchi, VauchiError>) {
    match result {
        Err(VauchiError::Storage(StorageError::WrongKey)) => {}
        Err(other) => panic!("expected WrongKey, got {other:?}"),
        Ok(_) => panic!("expected WrongKey, the database opened"),
    }
}

/// A database under `key` with an own card and, optionally, an identity —
/// what an install that kept its storage key in the shell left behind.
fn legacy_install(path: &Path, key: SymmetricKey, with_identity: bool) {
    let mut vauchi =
        Vauchi::new(VauchiConfig::with_storage_path(path).with_storage_key(key)).unwrap();
    if with_identity {
        vauchi.create_identity("Alice").unwrap();
    } else {
        vauchi
            .storage()
            .contacts()
            .save_own_card(&ContactCard::new("Alice"))
            .unwrap();
    }
}

// @internal
#[test]
fn data_written_before_an_identity_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let secure = Arc::new(MemoryKeyStorage::new());
    {
        let vauchi = boot(&path, &secure).unwrap();
        vauchi
            .storage()
            .contacts()
            .save_own_card(&ContactCard::new("Alice"))
            .unwrap();
    }

    let vauchi = boot(&path, &secure).unwrap();

    assert_eq!(own_card_name(&vauchi), Some("Alice".to_string()));
    assert!(secure.has_key(BOOTSTRAP_KEY_NAME).unwrap());
    assert!(!secure.has_key(SMK_KEY_NAME).unwrap());
}

// @internal
#[test]
fn creating_an_identity_replaces_the_bootstrap_key_with_the_smk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let secure = Arc::new(MemoryKeyStorage::new());
    {
        let mut vauchi = boot(&path, &secure).unwrap();
        vauchi.create_identity("Alice").unwrap();
    }

    assert!(!secure.has_key(BOOTSTRAP_KEY_NAME).unwrap());
    Storage::open(&path, sek_of(&secure)).unwrap();
    let vauchi = boot(&path, &secure).unwrap();
    assert_eq!(own_card_name(&vauchi), Some("Alice".to_string()));
}

// @internal
#[test]
fn a_shell_key_with_an_identity_is_migrated_to_the_smk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let shell_key = SymmetricKey::generate();
    legacy_install(&path, shell_key.clone(), true);
    let secure = Arc::new(MemoryKeyStorage::new());

    let vauchi = boot_with_shell_key(&path, &secure, shell_key.clone()).unwrap();

    assert_eq!(own_card_name(&vauchi), Some("Alice".to_string()));
    assert!(!secure.has_key(BOOTSTRAP_KEY_NAME).unwrap());
    drop(vauchi);
    assert!(matches!(
        Storage::open(&path, shell_key),
        Err(StorageError::WrongKey)
    ));
    Storage::open(&path, sek_of(&secure)).unwrap();
}

// @internal
#[test]
fn a_shell_key_without_an_identity_becomes_the_bootstrap_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let shell_key = SymmetricKey::generate();
    legacy_install(&path, shell_key.clone(), false);
    let secure = Arc::new(MemoryKeyStorage::new());
    drop(boot_with_shell_key(&path, &secure, shell_key.clone()).unwrap());

    let vauchi = boot(&path, &secure).unwrap();

    assert_eq!(own_card_name(&vauchi), Some("Alice".to_string()));
    assert_eq!(
        secure.load_key(BOOTSTRAP_KEY_NAME).unwrap(),
        Some(shell_key.as_bytes().to_vec())
    );
}

// @internal
#[test]
fn a_migration_interrupted_after_the_smk_was_saved_completes_at_boot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let bootstrap_key = SymmetricKey::generate();
    legacy_install(&path, bootstrap_key.clone(), true);
    let secure = Arc::new(MemoryKeyStorage::new());
    secure
        .save_key(BOOTSTRAP_KEY_NAME, bootstrap_key.as_bytes())
        .unwrap();
    let smk = Identity::create("Alice", 0).derive_smk();
    secure.save_key(SMK_KEY_NAME, smk.as_bytes()).unwrap();

    let vauchi = boot(&path, &secure).unwrap();

    assert_eq!(own_card_name(&vauchi), Some("Alice".to_string()));
    assert!(!secure.has_key(BOOTSTRAP_KEY_NAME).unwrap());
    drop(vauchi);
    Storage::open(&path, smk.derive_sek()).unwrap();
}

// @internal
#[test]
fn a_bootstrap_key_left_after_a_completed_migration_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let secure = Arc::new(MemoryKeyStorage::new());
    {
        let mut vauchi = boot(&path, &secure).unwrap();
        vauchi.create_identity("Alice").unwrap();
    }
    secure
        .save_key(BOOTSTRAP_KEY_NAME, SymmetricKey::generate().as_bytes())
        .unwrap();

    let vauchi = boot(&path, &secure).unwrap();

    assert_eq!(own_card_name(&vauchi), Some("Alice".to_string()));
    assert!(!secure.has_key(BOOTSTRAP_KEY_NAME).unwrap());
}

// @internal
#[test]
fn a_database_no_stored_key_opens_fails_closed_and_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    legacy_install(&path, SymmetricKey::generate(), true);
    let secure = Arc::new(MemoryKeyStorage::new());

    assert_wrong_key(boot(&path, &secure));
    assert_wrong_key(boot_with_shell_key(
        &path,
        &secure,
        SymmetricKey::generate(),
    ));

    assert!(!secure.has_key(BOOTSTRAP_KEY_NAME).unwrap());
    assert!(!secure.has_key(SMK_KEY_NAME).unwrap());
}

// @internal
#[test]
fn a_stored_key_of_the_wrong_length_is_refused_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let secure = Arc::new(MemoryKeyStorage::new());
    drop(boot(&path, &secure).unwrap());
    secure.save_key(BOOTSTRAP_KEY_NAME, &[7u8; 16]).unwrap();

    assert!(matches!(
        boot(&path, &secure),
        Err(VauchiError::Configuration(_))
    ));
    assert_eq!(
        secure.load_key(BOOTSTRAP_KEY_NAME).unwrap(),
        Some(vec![7u8; 16])
    );
}
