// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Vauchi::perform_emergency_wipe` on a real data directory is the
//! crypto-shred (vauchi/private#599): it succeeds only once the keys are gone.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::storage::SecureStorage;
use vauchi_core::{StorageError, VauchiError};

/// Platform secure storage; `deletes` false models a keychain that refuses
/// to delete.
struct Keychain {
    keys: Mutex<HashMap<String, Vec<u8>>>,
    deletes: bool,
}

impl Keychain {
    fn new(deletes: bool) -> Arc<Self> {
        Arc::new(Self {
            keys: Mutex::new(HashMap::new()),
            deletes,
        })
    }
}

impl SecureStorage for Keychain {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        self.keys.lock().unwrap().insert(name.into(), key.to_vec());
        Ok(())
    }
    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.keys.lock().unwrap().get(name).cloned())
    }
    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        if !self.deletes {
            return Err(StorageError::SecureStorageUnavailable);
        }
        self.keys.lock().unwrap().remove(name);
        Ok(())
    }
}

fn install(keychain: Arc<Keychain>) -> (tempfile::TempDir, std::path::PathBuf, Vauchi) {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = VauchiConfig::with_storage_path(data_dir.join("vauchi.db"));
    let mut vauchi = Vauchi::with_secure_storage(config, keychain).unwrap();
    vauchi.create_identity("Alice").unwrap();
    (root, data_dir, vauchi)
}

// @internal
#[test]
fn a_wipe_that_deletes_the_keys_succeeds_and_leaves_nothing() {
    let keychain = Keychain::new(true);
    let (_root, data_dir, mut vauchi) = install(keychain.clone());

    assert!(vauchi.perform_emergency_wipe(true).is_ok());

    assert!(keychain.keys.lock().unwrap().is_empty(), "keys survived");
    assert!(!data_dir.exists(), "the data directory survived");
    assert!(vauchi.identity().is_none());
}

// @internal
#[test]
fn a_wipe_whose_keys_cannot_be_deleted_reports_failure() {
    let keychain = Keychain::new(false);
    let (_root, _data_dir, mut vauchi) = install(keychain.clone());

    let result = vauchi.perform_emergency_wipe(true);

    assert!(
        matches!(result, Err(VauchiError::Configuration(_))),
        "a wipe that left the keys reported {result:?}"
    );
    assert!(keychain.keys.lock().unwrap().contains_key("smk"));
}

// @internal
#[test]
fn a_wipe_without_secure_storage_deletes_the_data_directory() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = VauchiConfig::with_storage_path(data_dir.join("vauchi.db"))
        .with_storage_key(vauchi_core::crypto::SymmetricKey::generate());
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.create_identity("Alice").unwrap();

    assert!(vauchi.perform_emergency_wipe(true).is_ok());

    assert!(!data_dir.exists(), "the data directory survived");
    assert!(vauchi.identity().is_none());
}
