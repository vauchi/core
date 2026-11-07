// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A scheduler run on a locked phone cannot open storage. It still fails as
//! before, so the shell's retry and chain stay as they are (#603), and Core
//! leaves a record the next unlocked open shows (vauchi/private#1093
//! slice 3).
//!
//! Traces to: features/sync_updates.feature

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use vauchi_platform::{KeychainError, MobilePlatformKeychain, PlatformAppEngine};

use crate::support::{SharedKeychain, drive_onboarding};

const RELAY: &str = "https://relay.test";
const MISSED_COPY: &str = "Couldn't check for updates while your phone was locked";

#[derive(Clone)]
struct LockableKeychain {
    store: SharedKeychain,
    locked: Arc<AtomicBool>,
}

impl MobilePlatformKeychain for LockableKeychain {
    fn save_key(&self, name: String, key: Vec<u8>) -> Result<(), KeychainError> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(KeychainError::AuthenticationRequired);
        }
        self.store.save_key(name, key)
    }
    fn load_key(&self, name: String) -> Result<Option<Vec<u8>>, KeychainError> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(KeychainError::AuthenticationRequired);
        }
        self.store.load_key(name)
    }
    fn delete_key(&self, name: String) -> Result<(), KeychainError> {
        self.store.delete_key(name)
    }
}

fn open(dir: &tempfile::TempDir, keychain: &LockableKeychain) -> Arc<PlatformAppEngine> {
    PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        None,
        Box::new(keychain.clone()),
    )
    .expect("a locked keychain still yields an engine")
}

fn onboarded(dir: &tempfile::TempDir) -> LockableKeychain {
    let keychain = LockableKeychain {
        store: SharedKeychain::new(),
        locked: Arc::new(AtomicBool::new(false)),
    };
    drive_onboarding(&open(dir, &keychain));
    keychain
}

// @scenario: sync_updates :: A background check missed while the phone was locked shows after unlock
#[test]
fn a_locked_scheduler_run_is_shown_at_the_next_unlocked_open() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.locked.store(true, Ordering::SeqCst);
    let worker = open(&dir, &keychain);

    assert!(
        worker.periodic_sync_tick().is_err(),
        "a locked run still fails, as the shells expect"
    );
    drop(worker);
    keychain.locked.store(false, Ordering::SeqCst);
    let app = open(&dir, &keychain);

    let batch = app.initial_commands_json().unwrap();
    assert!(batch.contains(MISSED_COPY), "got {batch}");
}

// @internal
#[test]
fn an_unlocked_open_alone_records_no_missed_run() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    drop(open(&dir, &keychain));

    let app = open(&dir, &keychain);

    let batch = app.initial_commands_json().unwrap();
    assert!(!batch.contains(MISSED_COPY), "got {batch}");
}

// @internal
#[test]
fn a_locked_open_without_a_scheduler_run_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.locked.store(true, Ordering::SeqCst);
    let foreground = open(&dir, &keychain);
    foreground.initial_commands_json().unwrap();
    drop(foreground);
    keychain.locked.store(false, Ordering::SeqCst);

    let app = open(&dir, &keychain);

    let batch = app.initial_commands_json().unwrap();
    assert!(!batch.contains(MISSED_COPY), "got {batch}");
}
