// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A keychain that needs the person to authenticate, or lost its key, no
//! longer fails engine construction: the engine starts locked and shows
//! Core's unlock or recovery screen through the normal presentation calls
//! (vauchi/private#580, ADR-043 Amendment 7 §3). Nothing is deleted until
//! the person confirms.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use vauchi_platform::{KeychainError, MobilePlatformKeychain, PlatformAppEngine};

use crate::support::{SharedKeychain, drive_onboarding};

const RELAY: &str = "https://relay.test";

/// A keychain the test can lock and that can lose the key named "smk".
#[derive(Clone)]
struct ControlledKeychain {
    store: SharedKeychain,
    locked: Arc<AtomicBool>,
    smk_invalidated: Arc<AtomicBool>,
}

impl ControlledKeychain {
    fn new(store: SharedKeychain) -> Self {
        Self {
            store,
            locked: Arc::new(AtomicBool::new(false)),
            smk_invalidated: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl MobilePlatformKeychain for ControlledKeychain {
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
        if name == "smk" && self.smk_invalidated.load(Ordering::SeqCst) {
            return Err(KeychainError::KeyInvalidated);
        }
        self.store.load_key(name)
    }
    fn delete_key(&self, name: String) -> Result<(), KeychainError> {
        if name == "smk" {
            self.smk_invalidated.store(false, Ordering::SeqCst);
        }
        self.store.delete_key(name)
    }
}

fn open(dir: &tempfile::TempDir, keychain: &ControlledKeychain) -> Arc<PlatformAppEngine> {
    PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        None,
        Box::new(keychain.clone()),
    )
    .expect("a locked or unreadable keychain still yields an engine")
}

fn parse(batch: String) -> Value {
    serde_json::from_str(&batch).expect("command batch json")
}

fn command_names(batch: &Value) -> Vec<String> {
    batch["commands"]
        .as_array()
        .expect("commands array")
        .iter()
        .map(|command| match command {
            Value::String(name) => name.clone(),
            Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
            _ => String::new(),
        })
        .collect()
}

fn asks_for_prompt(batch: &Value) -> bool {
    command_names(batch).contains(&"RequestBiometricUnlock".to_string())
}

fn surface(batch: &Value) -> String {
    batch["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("SetContextBar"))
        .and_then(|bar| bar["surface_id"].as_str())
        .expect("a context bar")
        .to_owned()
}

fn press_primary(engine: &PlatformAppEngine, batch: &Value) -> Value {
    let bar = batch["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("SetContextBar"))
        .expect("a context bar");
    let event = serde_json::json!({
        "ActionActivated": {
            "surface_id": bar["surface_id"],
            "interaction_id": bar["bar"]["primary"]["interaction_id"],
        }
    });
    parse(engine.dispatch_json(event.to_string()).expect("dispatch"))
}

fn prompt_succeeded(engine: &PlatformAppEngine) -> Value {
    let event = serde_json::to_string(&vauchi_core::Event::BiometricUnlockSucceeded).unwrap();
    parse(engine.dispatch_json(event).expect("dispatch"))
}

/// An install that finished onboarding, so the keychain holds the SMK.
fn onboarded(dir: &tempfile::TempDir) -> ControlledKeychain {
    let keychain = ControlledKeychain::new(SharedKeychain::new());
    drive_onboarding(&open(dir, &keychain));
    keychain
}

// @internal
#[test]
fn a_locked_keychain_starts_a_locked_engine_that_asks_for_the_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.locked.store(true, Ordering::SeqCst);

    let engine = open(&dir, &keychain);

    let batch = parse(engine.initial_commands_json().unwrap());
    assert!(asks_for_prompt(&batch));
    assert_eq!(surface(&batch), "storage_lock.locked");
    assert!(engine.has_identity().is_err(), "storage is not open yet");
}

// @internal
#[test]
fn storage_opens_once_the_prompt_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.locked.store(true, Ordering::SeqCst);
    let engine = open(&dir, &keychain);
    engine.initial_commands_json().unwrap();

    let still_locked = prompt_succeeded(&engine);
    assert!(
        asks_for_prompt(&still_locked),
        "a retry while locked prompts again"
    );

    keychain.locked.store(false, Ordering::SeqCst);
    let opened = prompt_succeeded(&engine);

    assert!(
        !surface(&opened).starts_with("storage_lock"),
        "got {opened}"
    );
    assert_eq!(engine.has_identity().unwrap(), true);
}

// @internal
#[test]
fn a_lost_key_shows_recovery_and_deletes_nothing_until_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.smk_invalidated.store(true, Ordering::SeqCst);
    let db = dir.path().join("vauchi.db");
    let db_before = std::fs::metadata(&db).unwrap().len();

    let engine = open(&dir, &keychain);
    let unreadable = parse(engine.initial_commands_json().unwrap());
    assert_eq!(surface(&unreadable), "storage_lock.unreadable");
    assert!(!asks_for_prompt(&unreadable));
    let confirm = press_primary(&engine, &unreadable);
    assert_eq!(surface(&confirm), "storage_lock.confirm_start_over");

    assert_eq!(std::fs::metadata(&db).unwrap().len(), db_before);
    assert_eq!(keychain.store.names(), ["smk"]);
}

// @internal
#[test]
fn starting_over_deletes_the_unreadable_data_and_opens_a_fresh_install() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.smk_invalidated.store(true, Ordering::SeqCst);
    let engine = open(&dir, &keychain);
    let unreadable = parse(engine.initial_commands_json().unwrap());
    let confirm = press_primary(&engine, &unreadable);

    let fresh = press_primary(&engine, &confirm);

    assert!(!surface(&fresh).starts_with("storage_lock"), "got {fresh}");
    assert_eq!(engine.has_identity().unwrap(), false);
    assert_eq!(keychain.store.names(), ["storage_bootstrap"]);
}

// @internal
#[test]
fn data_no_stored_key_opens_shows_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    let empty = ControlledKeychain::new(SharedKeychain::new());

    let engine = open(&dir, &empty);

    let batch = parse(engine.initial_commands_json().unwrap());
    assert_eq!(surface(&batch), "storage_lock.unreadable");
    assert_eq!(keychain.store.names(), ["smk"]);
    assert_eq!(empty.store.names(), Vec::<String>::new());
}
