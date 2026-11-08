// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Background Receive Key (ADR-033 amendment 2026-11-07): random, made
//! with the identity or at the first open of an older one, kept in the
//! class usable after the first unlock since boot, and destroyed by every
//! shred with the SMK (vauchi/private#1093 slice 4).
//!
//! Traces to: features/storage_key_upgrade.feature, features/emergency_shred.feature

use std::path::Path;
use std::sync::Arc;

use vauchi_core::api::storage_reset::delete_unreadable_data;
use vauchi_core::api::{ShredManager, Vauchi, VauchiConfig, widget_panic_shred};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::{KeyAccess, MemoryKeyStorage, SecureStorage, Storage, StorageError};

const SMK: &str = "smk";
const BRK: &str = "brk";

/// A keychain that keeps each access class apart, as iOS and Android do,
/// so a test sees which class a key went to.
#[derive(Default)]
struct ClassedKeychain {
    after_authentication: MemoryKeyStorage,
    after_first_unlock: MemoryKeyStorage,
    refuse_after_first_unlock: bool,
}

impl ClassedKeychain {
    fn class(&self, access: KeyAccess) -> &MemoryKeyStorage {
        match access {
            KeyAccess::AfterAuthentication => &self.after_authentication,
            KeyAccess::AfterFirstUnlock => &self.after_first_unlock,
        }
    }

    fn brk(&self) -> Option<Vec<u8>> {
        self.after_first_unlock.load_key(BRK).unwrap()
    }
}

impl SecureStorage for ClassedKeychain {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        self.after_authentication.save_key(name, key)
    }
    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.after_authentication.load_key(name)
    }
    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        self.after_authentication.delete_key(name)
    }
    fn save_key_with_access(
        &self,
        name: &str,
        key: &[u8],
        access: KeyAccess,
    ) -> Result<(), StorageError> {
        if self.refuse_after_first_unlock && access == KeyAccess::AfterFirstUnlock {
            return Err(StorageError::SecureStorageUnavailable);
        }
        self.class(access).save_key(name, key)
    }
    fn load_key_with_access(
        &self,
        name: &str,
        access: KeyAccess,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.class(access).load_key(name)
    }
    fn secure_delete_key_with_access(
        &self,
        name: &str,
        access: KeyAccess,
    ) -> Result<(), StorageError> {
        self.class(access).delete_key(name)
    }
}

fn open(path: &Path, keychain: &Arc<ClassedKeychain>) -> Vauchi {
    Vauchi::with_secure_storage(VauchiConfig::with_storage_path(path), keychain.clone())
        .expect("storage opens")
}

fn created(dir: &Path) -> Arc<ClassedKeychain> {
    let keychain = Arc::new(ClassedKeychain::default());
    open(&dir.join("vauchi.db"), &keychain)
        .create_identity("Alice")
        .expect("identity");
    keychain
}

// @scenario: storage_key_upgrade :: Creating an identity makes a background receive key
#[test]
fn creating_an_identity_keeps_a_brk_usable_after_the_first_unlock() {
    let dir = tempfile::tempdir().unwrap();

    let keychain = created(dir.path());

    assert_eq!(keychain.brk().map(|key| key.len()), Some(32));
    assert_eq!(
        keychain.after_authentication.load_key(BRK).unwrap(),
        None,
        "the BRK must not sit in the class that needs authentication"
    );
}

// @internal
#[test]
fn the_brk_is_random_not_derived_from_the_smk() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();

    let a = created(first.path());
    let b = created(second.path());

    assert_ne!(a.brk(), b.brk());
    assert_ne!(a.brk(), a.after_authentication.load_key(SMK).unwrap());
}

// @scenario: storage_key_upgrade :: An identity made before the background receive key gets one at the next open
#[test]
fn an_identity_without_a_brk_gets_one_at_the_next_open_and_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = created(dir.path());
    keychain
        .secure_delete_key_with_access(BRK, KeyAccess::AfterFirstUnlock)
        .unwrap();

    drop(open(&dir.path().join("vauchi.db"), &keychain));
    let made_at_open = keychain.brk();
    drop(open(&dir.path().join("vauchi.db"), &keychain));

    assert_eq!(made_at_open.as_ref().map(Vec::len), Some(32));
    assert_eq!(keychain.brk(), made_at_open, "a second open keeps the BRK");
}

// @internal
#[test]
fn a_brk_that_cannot_be_stored_does_not_stop_the_open() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = created(dir.path());
    let refusing = Arc::new(ClassedKeychain {
        refuse_after_first_unlock: true,
        ..ClassedKeychain::default()
    });
    let smk = keychain
        .after_authentication
        .load_key(SMK)
        .unwrap()
        .unwrap();
    refusing.after_authentication.save_key(SMK, &smk).unwrap();

    let vauchi = open(&dir.path().join("vauchi.db"), &refusing);

    assert!(vauchi.has_identity());
    assert_eq!(refusing.brk(), None);
}

/// A keychain holding an SMK and a BRK, and the data directory a shred
/// destroys.
fn keys_and_data(dir: &Path) -> (ClassedKeychain, Storage, Identity) {
    let keychain = ClassedKeychain::default();
    let identity = Identity::create("Alice", 0);
    keychain
        .save_key(SMK, identity.derive_smk().as_bytes())
        .unwrap();
    keychain
        .save_key_with_access(
            BRK,
            SymmetricKey::generate().as_bytes(),
            KeyAccess::AfterFirstUnlock,
        )
        .unwrap();
    let storage = Storage::open(dir.join("vauchi.db"), SymmetricKey::generate()).unwrap();
    (keychain, storage, identity)
}

// @claim: S12
// @scenario: emergency_shred :: Every shred destroys the background receive key
#[test]
fn every_shred_destroys_the_brk_with_the_smk() {
    type Shred = fn(&Storage, &ClassedKeychain, &Identity, &Path);
    let shreds: [(&str, Shred); 2] = [
        ("panic", |storage, keychain, identity, dir| {
            ShredManager::new(storage, keychain, identity, dir)
                .panic_shred()
                .unwrap();
        }),
        ("widget", |_, keychain, _, dir| {
            widget_panic_shred(dir, keychain).unwrap();
        }),
    ];
    for (name, shred) in shreds {
        let dir = tempfile::tempdir().unwrap();
        let (keychain, storage, identity) = keys_and_data(dir.path());

        shred(&storage, &keychain, &identity, dir.path());

        assert_eq!(keychain.brk(), None, "{name} shred left the BRK");
        assert_eq!(
            keychain.after_authentication.load_key(SMK).unwrap(),
            None,
            "{name} shred left the SMK"
        );
    }
}

// @internal
#[test]
fn a_shred_is_not_verified_clear_while_a_brk_remains() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let (keychain, storage, identity) = keys_and_data(&data_dir);
    let manager = ShredManager::new(&storage, &keychain, &identity, &data_dir);
    manager.panic_shred().unwrap();
    assert!(manager.verify_shred().all_clear);

    keychain
        .save_key_with_access(
            BRK,
            SymmetricKey::generate().as_bytes(),
            KeyAccess::AfterFirstUnlock,
        )
        .unwrap();

    let verification = manager.verify_shred();
    assert!(!verification.brk_absent);
    assert!(!verification.all_clear);
}

// @internal
#[test]
fn starting_over_deletes_the_brk() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = created(dir.path());

    delete_unreadable_data(&dir.path().join("vauchi.db"), keychain.as_ref()).unwrap();

    assert_eq!(keychain.brk(), None);
}
