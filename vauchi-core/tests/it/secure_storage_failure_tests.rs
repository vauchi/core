// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A keychain that is locked or lost its key says so to Core, which picks
//! the screen (ADR-045, vauchi/private#580). Neither failure may make Core
//! create, replace or delete a key: a locked keychain unlocks later, and a
//! lost key is the user's decision to act on.

use std::sync::{Arc, Mutex};

use vauchi_core::api::{Vauchi, VauchiConfig, VauchiError};
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage, StorageError};

/// A keychain whose reads fail with `failure`; writes and deletes are
/// recorded so a test can prove none happened.
struct FailingKeychain {
    failure: fn() -> StorageError,
    writes: Mutex<Vec<String>>,
}

impl FailingKeychain {
    fn new(failure: fn() -> StorageError) -> Self {
        Self {
            failure,
            writes: Mutex::new(Vec::new()),
        }
    }
}

impl SecureStorage for FailingKeychain {
    fn save_key(&self, name: &str, _key: &[u8]) -> Result<(), StorageError> {
        self.writes.lock().unwrap().push(format!("save {name}"));
        Ok(())
    }

    fn load_key(&self, _name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Err((self.failure)())
    }

    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        self.writes.lock().unwrap().push(format!("delete {name}"));
        Ok(())
    }
}

fn boot(keychain: Arc<FailingKeychain>) -> (tempfile::TempDir, Result<Vauchi, VauchiError>) {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"));
    let result = Vauchi::with_secure_storage(config, keychain);
    (dir, result)
}

fn failures() -> [(&'static str, fn() -> StorageError); 2] {
    [
        ("locked", || StorageError::SecureStorageLocked),
        ("key invalidated", || {
            StorageError::SecureStorageKeyInvalidated
        }),
    ]
}

// @internal
#[test]
fn a_keychain_failure_reaches_core_as_its_own_kind() {
    for (name, failure) in failures() {
        let (_dir, result) = boot(Arc::new(FailingKeychain::new(failure)));

        let expected = failure();
        match result {
            Err(VauchiError::Storage(actual))
                if std::mem::discriminant(&actual) == std::mem::discriminant(&expected) => {}
            Err(other) => panic!("{name}: expected {expected:?}, got {other:?}"),
            Ok(_) => panic!("{name}: expected {expected:?}, the database opened"),
        }
    }
}

// @internal
#[test]
fn a_keychain_failure_creates_replaces_or_deletes_no_key() {
    for (name, failure) in failures() {
        let keychain = Arc::new(FailingKeychain::new(failure));

        let (_dir, result) = boot(keychain.clone());

        assert!(result.is_err(), "{name}: boot must not succeed");
        assert_eq!(
            *keychain.writes.lock().unwrap(),
            Vec::<String>::new(),
            "{name}: no key may be written or deleted"
        );
    }
}

// @internal
#[test]
fn a_locked_keychain_is_not_unreadable_storage() {
    let locked = VauchiError::Storage(StorageError::SecureStorageLocked);
    let lost = VauchiError::Storage(StorageError::SecureStorageKeyInvalidated);

    assert!(!locked.is_unreadable_storage());
    assert!(lost.is_unreadable_storage());
}

// @internal
#[test]
fn an_unlocked_keychain_still_boots() {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"));

    let vauchi = Vauchi::with_secure_storage(config, Arc::new(MemoryKeyStorage::new()));

    assert!(vauchi.is_ok(), "{:?}", vauchi.err());
}
