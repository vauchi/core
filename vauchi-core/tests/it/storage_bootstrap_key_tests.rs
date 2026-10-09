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

use vauchi_core::api::{ShredManager, Vauchi, VauchiConfig, VauchiError, widget_panic_shred};
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

/// Secure storage holding an SMK and a bootstrap key, as a crash between
/// the SMK move and the bootstrap delete leaves it.
fn keys_with_leftover_bootstrap() -> MemoryKeyStorage {
    let secure = MemoryKeyStorage::new();
    let smk = Identity::create("Alice", 0).derive_smk();
    secure.save_key(SMK_KEY_NAME, smk.as_bytes()).unwrap();
    secure
        .save_key(BOOTSTRAP_KEY_NAME, SymmetricKey::generate().as_bytes())
        .unwrap();
    secure
}

// @internal
#[test]
fn every_shred_deletes_the_bootstrap_key() {
    type Shred = fn(&Storage, &MemoryKeyStorage, &Identity, &Path);
    let shreds: [(&str, Shred); 2] = [
        ("panic", |storage, secure, identity, dir| {
            ShredManager::new(storage, secure, identity, dir)
                .panic_shred(None, None)
                .unwrap();
        }),
        ("widget", |_, secure, _, dir| {
            widget_panic_shred(dir, secure).unwrap();
        }),
    ];
    for (name, shred) in shreds {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            Storage::open(dir.path().join("vauchi.db"), SymmetricKey::generate()).unwrap();
        let secure = keys_with_leftover_bootstrap();
        let identity = Identity::create("Alice", 0);

        shred(&storage, &secure, &identity, dir.path());

        assert!(
            !secure.has_key(BOOTSTRAP_KEY_NAME).unwrap(),
            "{name} shred left the bootstrap key"
        );
        assert!(
            !secure.has_key(SMK_KEY_NAME).unwrap(),
            "{name} shred left the SMK"
        );
    }
}

// @internal
#[test]
fn a_shred_is_not_verified_clear_while_a_bootstrap_key_remains() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let storage = Storage::open(data_dir.join("vauchi.db"), SymmetricKey::generate()).unwrap();
    let secure = keys_with_leftover_bootstrap();
    let identity = Identity::create("Alice", 0);
    let manager = ShredManager::new(&storage, &secure, &identity, &data_dir);
    manager.panic_shred(None, None).unwrap();
    assert!(manager.verify_shred().all_clear);

    secure
        .save_key(BOOTSTRAP_KEY_NAME, SymmetricKey::generate().as_bytes())
        .unwrap();

    let verification = manager.verify_shred();
    assert!(!verification.bootstrap_key_absent);
    assert!(!verification.all_clear);
}

/// Secure storage whose delete of one key name fails.
struct UndeletableKey {
    inner: MemoryKeyStorage,
    undeletable: &'static str,
}

impl SecureStorage for UndeletableKey {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        self.inner.save_key(name, key)
    }
    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.inner.load_key(name)
    }
    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        if name == self.undeletable {
            return Err(StorageError::SecureStorageLocked);
        }
        self.inner.delete_key(name)
    }
}

// @internal
#[test]
fn a_shred_that_could_not_delete_every_storage_key_does_not_report_them_destroyed() {
    for undeletable in [SMK_KEY_NAME, BOOTSTRAP_KEY_NAME] {
        let dir = tempfile::tempdir().unwrap();
        let secure = UndeletableKey {
            inner: keys_with_leftover_bootstrap(),
            undeletable,
        };

        let report = widget_panic_shred(dir.path(), &secure).unwrap();

        assert!(
            !report.smk_destroyed,
            "{undeletable} survived but the report says the keys are destroyed"
        );
    }
}

// @internal
#[test]
fn storage_in_a_directory_that_does_not_exist_yet_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not").join("yet").join("vauchi.db");

    let with_keychain = boot(&path, &Arc::new(MemoryKeyStorage::new()));
    let without_keychain = Vauchi::new(
        VauchiConfig::with_storage_path(dir.path().join("other").join("vauchi.db"))
            .with_storage_key(SymmetricKey::generate()),
    );

    assert!(with_keychain.is_ok(), "{:?}", with_keychain.err());
    assert!(without_keychain.is_ok(), "{:?}", without_keychain.err());
    assert!(path.exists());
}

/// Secure storage whose first save fails as if the keychain were locked.
struct FirstSaveLocked {
    inner: MemoryKeyStorage,
    failed_once: std::sync::atomic::AtomicBool,
}

impl SecureStorage for FirstSaveLocked {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        if !self
            .failed_once
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(StorageError::SecureStorageLocked);
        }
        self.inner.save_key(name, key)
    }
    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.inner.load_key(name)
    }
    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        self.inner.delete_key(name)
    }
}

// @internal
#[test]
fn a_fresh_install_that_could_not_save_its_key_starts_cleanly_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let secure = Arc::new(FirstSaveLocked {
        inner: MemoryKeyStorage::new(),
        failed_once: std::sync::atomic::AtomicBool::new(false),
    });
    let config = || VauchiConfig::with_storage_path(&path);

    let first = Vauchi::with_secure_storage(config(), secure.clone());
    assert!(matches!(
        first,
        Err(VauchiError::Storage(StorageError::SecureStorageLocked))
    ));

    let second = Vauchi::with_secure_storage(config(), secure.clone());

    assert!(second.is_ok(), "{:?}", second.err());
    assert!(secure.inner.has_key(BOOTSTRAP_KEY_NAME).unwrap());
}
