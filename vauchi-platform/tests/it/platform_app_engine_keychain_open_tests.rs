// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mobile shells open Core with the platform keychain from the first
//! call (`PlatformAppEngine::open_with_keychain`), so every key that opens the database lives
//! in the keychain and a shred deletes them all (ADR-033,
//! vauchi/private#580). The keychain is platform secure storage, so an
//! in-memory fake is the right double; the crypto is real (ADR-002).

use std::sync::Arc;

use vauchi_core::crypto::SymmetricKey;
use vauchi_platform::{
    DomainCommand, DomainCommandResult, HandedOverSecret, KeychainError, MobilePlatformKeychain,
    PlatformAppEngine,
};

use crate::support::{SharedKeychain, drive_onboarding};

const RELAY: &str = "https://relay.test";

fn start(
    dir: &tempfile::TempDir,
    shell_key: Option<&SymmetricKey>,
    keychain: &SharedKeychain,
) -> Arc<PlatformAppEngine> {
    PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        shell_key.map(|key| HandedOverSecret {
            handle: "legacy-storage-key".into(),
            secret: key.as_bytes().to_vec(),
        }),
        Box::new(keychain.clone()),
    )
    .expect("open engine")
}

// @internal
#[test]
fn a_fresh_install_keeps_its_storage_key_in_the_keychain() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();

    let engine = start(&dir, None, &keychain);

    assert!(engine.initial_commands_json().unwrap().contains("commands"));
    assert_eq!(keychain.names(), ["storage_bootstrap"]);
}

// @internal
#[test]
fn a_shell_key_is_adopted_and_moved_to_the_smk_for_an_existing_identity() {
    let dir = tempfile::tempdir().unwrap();
    let shell_key = SymmetricKey::generate();
    {
        let legacy = PlatformAppEngine::new(
            dir.path().to_string_lossy().to_string(),
            RELAY.into(),
            shell_key.as_bytes().to_vec(),
        )
        .unwrap();
        drive_onboarding(&legacy);
    }
    let keychain = SharedKeychain::new();

    let engine = start(&dir, Some(&shell_key), &keychain);

    assert_eq!(keychain.names(), ["smk"]);
    assert!(engine.initial_commands_json().unwrap().contains("commands"));
}

// @internal
#[test]
fn after_onboarding_a_panic_shred_leaves_no_key_in_the_keychain() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = start(&dir, None, &keychain);
    drive_onboarding(&engine);
    assert_eq!(keychain.names(), ["smk"]);

    let result = engine
        .dispatch_domain_command(DomainCommand::PanicShred)
        .expect("panic shred without a separate set_platform_keychain call");

    assert!(
        matches!(result, DomainCommandResult::ShredCompleted { ref report } if report.sqlite_destroyed),
        "got {result:?}"
    );
    assert_eq!(keychain.names(), Vec::<String>::new());
}

// @internal
#[test]
fn a_malformed_shell_key_is_refused_before_the_keychain_is_touched() {
    for bad in [vec![1u8; 16], vec![0u8; 32], Vec::new()] {
        let dir = tempfile::tempdir().unwrap();
        let keychain = SharedKeychain::new();

        let result = PlatformAppEngine::open_with_keychain(
            dir.path().to_string_lossy().to_string(),
            RELAY.into(),
            Some(HandedOverSecret {
                handle: "legacy-storage-key".into(),
                secret: bad.clone(),
            }),
            Box::new(keychain.clone()),
        );

        assert!(result.is_err(), "{} bytes must be refused", bad.len());
        assert_eq!(keychain.names(), Vec::<String>::new());
    }
}

struct BrokenKeychain;

impl MobilePlatformKeychain for BrokenKeychain {
    fn save_key(&self, _: String, _: Vec<u8>) -> Result<(), KeychainError> {
        panic!("a failed read must not lead to a write")
    }
    fn load_key(&self, _: String) -> Result<Option<Vec<u8>>, KeychainError> {
        Err(KeychainError::OperationFailed {
            msg: "keystore unavailable".into(),
        })
    }
    fn delete_key(&self, _: String) -> Result<(), KeychainError> {
        panic!("a failed read must not lead to a delete")
    }
}

// @internal
#[test]
fn a_keychain_that_cannot_be_read_starts_an_engine_that_offers_try_again() {
    let dir = tempfile::tempdir().unwrap();

    let engine = PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        None,
        Box::new(BrokenKeychain),
    )
    .expect("an unavailable keychain still yields an engine");

    let batch = engine.initial_commands_json().unwrap();
    assert!(
        batch.contains("storage_lock.unavailable"),
        "expected the unavailable screen, got {batch}"
    );
    assert!(!dir.path().join("vauchi.db").exists());
}

fn forgets(batch: &str) -> Vec<String> {
    let batch: serde_json::Value = serde_json::from_str(batch).expect("batch json");
    batch["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .filter_map(|command| command.get("ForgetStoredSecret"))
        .map(|forget| forget["handle"].as_str().unwrap_or_default().to_owned())
        .collect()
}

// @internal
#[test]
fn a_handed_over_key_is_released_once_and_only_after_storage_opened() {
    let dir = tempfile::tempdir().unwrap();
    let shell_key = SymmetricKey::generate();
    drop(
        PlatformAppEngine::new(
            dir.path().to_string_lossy().to_string(),
            RELAY.into(),
            shell_key.as_bytes().to_vec(),
        )
        .unwrap(),
    );
    let keychain = SharedKeychain::new();
    let engine = start(&dir, Some(&shell_key), &keychain);

    let first = engine.initial_commands_json().unwrap();
    let second = engine.initial_commands_json().unwrap();

    assert_eq!(forgets(&first), ["legacy-storage-key"]);
    assert_eq!(forgets(&second), Vec::<String>::new());
    assert_eq!(keychain.names(), ["storage_bootstrap"]);
}

// @internal
#[test]
fn without_a_handed_over_key_nothing_is_released() {
    let dir = tempfile::tempdir().unwrap();
    let engine = start(&dir, None, &SharedKeychain::new());

    assert_eq!(
        forgets(&engine.initial_commands_json().unwrap()),
        Vec::<String>::new()
    );
}
