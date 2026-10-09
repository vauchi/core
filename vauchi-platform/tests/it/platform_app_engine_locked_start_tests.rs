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

fn activate(engine: &PlatformAppEngine, surface_id: &Value, interaction_id: &Value) -> Value {
    let event = serde_json::json!({
        "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
    });
    parse(engine.dispatch_json(event.to_string()).expect("dispatch"))
}

/// Opens the action menu and activates its only item, Start over.
fn choose_start_over(engine: &PlatformAppEngine, batch: &Value) -> Value {
    let bar = batch["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("SetContextBar"))
        .expect("a context bar")
        .clone();
    let menu = activate(
        engine,
        &bar["surface_id"],
        &bar["bar"]["secondary"]["interaction_id"],
    );
    let item = menu["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("PresentOverlay"))
        .expect("the action menu")["overlay"]["items"][0]
        .clone();
    activate(engine, &bar["surface_id"], &item["interaction_id"])
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
    assert_eq!(surface(&still_locked), "storage_lock.locked");
    assert!(
        !asks_for_prompt(&still_locked),
        "a prompt that did not unlock storage must not loop"
    );

    keychain.locked.store(false, Ordering::SeqCst);
    let opened = prompt_succeeded(&engine);

    assert!(
        !surface(&opened).starts_with("storage_lock"),
        "got {opened}"
    );
    assert!(engine.has_identity().unwrap());
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
    let confirm = choose_start_over(&engine, &unreadable);
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
    let confirm = choose_start_over(&engine, &unreadable);

    let fresh = press_primary(&engine, &confirm);

    assert!(!surface(&fresh).starts_with("storage_lock"), "got {fresh}");
    assert!(!engine.has_identity().unwrap());
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

fn has_alert(batch: &Value) -> bool {
    command_names(batch).contains(&"PresentAlert".to_string())
}

/// The confirmation screen of an install whose SMK the platform invalidated.
fn at_start_over_confirmation(
    dir: &tempfile::TempDir,
) -> (Arc<PlatformAppEngine>, ControlledKeychain, Value) {
    let keychain = onboarded(dir);
    keychain.smk_invalidated.store(true, Ordering::SeqCst);
    let engine = open(dir, &keychain);
    let unreadable = parse(engine.initial_commands_json().unwrap());
    let confirm = choose_start_over(&engine, &unreadable);
    (engine, keychain, confirm)
}

// @internal
#[test]
fn starting_over_removes_the_old_identity_files_too() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _keychain, confirm) = at_start_over_confirmation(&dir);
    let identity = dir.path().join("identity.json");
    let keys = dir.path().join("keys");
    let pre_signed = vauchi_core::api::PreSignedShredMessages::file_path(dir.path());
    std::fs::write(&identity, b"old identity").unwrap();
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(keys.join("old.key"), b"old key").unwrap();
    std::fs::write(&pre_signed, b"old pre-signed purge").unwrap();

    press_primary(&engine, &confirm);

    assert!(!identity.exists());
    assert!(!keys.exists());
    assert!(!pre_signed.exists());
}

// @internal
#[test]
fn a_start_over_that_cannot_remove_a_companion_file_keeps_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, keychain, confirm) = at_start_over_confirmation(&dir);
    let db = dir.path().join("vauchi.db");
    let wal = dir.path().join("vauchi.db-wal");
    let _ = std::fs::remove_file(&wal);
    std::fs::create_dir(&wal).unwrap();

    let answer = press_primary(&engine, &confirm);

    assert!(has_alert(&answer), "got {answer}");
    assert!(
        db.exists(),
        "the database must not be deleted while its WAL stays"
    );
    assert_eq!(keychain.store.names(), ["smk"]);
}

// @internal
#[test]
fn a_retry_that_fails_for_another_reason_answers_with_an_alert() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.locked.store(true, Ordering::SeqCst);
    let engine = open(&dir, &keychain);
    engine.initial_commands_json().unwrap();
    let db = dir.path().join("vauchi.db");
    std::fs::remove_file(&db).unwrap();
    std::fs::create_dir(&db).unwrap();
    keychain.locked.store(false, Ordering::SeqCst);

    let event = serde_json::to_string(&vauchi_core::Event::BiometricUnlockSucceeded).unwrap();
    let answer = engine.dispatch_json(event);

    let batch = parse(answer.expect("Core answers with commands, not an error"));
    assert!(has_alert(&batch), "got {batch}");
}

struct SilentListener;

impl vauchi_platform::PlatformEventListener for SilentListener {
    fn on_presentation_invalidated(&self) {}
}

fn german(key: &str) -> String {
    vauchi_platform::init_locales(crate::support::locales_dir().to_string_lossy().into_owned())
        .expect("locales");
    vauchi_app::i18n::get_string(vauchi_app::i18n::Locale::German, key)
}

const GERMAN_CONTEXT: &str = r#"{"locale":"de","theme_id":null}"#;

// @internal
#[test]
fn setup_calls_made_while_locked_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.locked.store(true, Ordering::SeqCst);
    let engine = open(&dir, &keychain);

    engine
        .set_render_context_json(GERMAN_CONTEXT.into())
        .expect("render context while locked");
    engine
        .set_device_capabilities_json("{}".into())
        .expect("capabilities while locked");
    engine
        .set_network_online(false)
        .expect("network state while locked");
    engine
        .set_event_listener(Box::new(SilentListener))
        .expect("listener while locked");
}

// @internal
#[test]
fn the_lock_screen_speaks_the_render_context_language() {
    let title = german("storage_lock.locked_title");
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.locked.store(true, Ordering::SeqCst);
    let engine = open(&dir, &keychain);

    engine
        .set_render_context_json(GERMAN_CONTEXT.into())
        .unwrap();
    let batch = engine.initial_commands_json().unwrap();

    assert!(batch.contains(&title), "expected {title:?} in {batch}");
}

// @internal
#[test]
fn a_render_context_set_while_locked_reaches_the_opened_engine() {
    let more = german("nav.more");
    let dir = tempfile::tempdir().unwrap();
    let keychain = onboarded(&dir);
    keychain.locked.store(true, Ordering::SeqCst);
    let engine = open(&dir, &keychain);
    engine
        .set_render_context_json(GERMAN_CONTEXT.into())
        .unwrap();
    engine.initial_commands_json().unwrap();

    keychain.locked.store(false, Ordering::SeqCst);
    let opened = prompt_succeeded(&engine).to_string();

    assert!(opened.contains(&more), "expected {more:?} in {opened}");
}

fn released(batch: &Value) -> Vec<String> {
    batch["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|command| command.get("ForgetStoredSecret"))
        .map(|forget| forget["handle"].as_str().unwrap_or_default().to_owned())
        .collect()
}

// @internal
#[test]
fn a_handed_over_key_is_not_released_while_storage_is_locked() {
    let dir = tempfile::tempdir().unwrap();
    let shell_key = vauchi_core::crypto::SymmetricKey::generate();
    drive_onboarding(
        &PlatformAppEngine::new(
            dir.path().to_string_lossy().to_string(),
            RELAY.into(),
            shell_key.as_bytes().to_vec(),
        )
        .unwrap(),
    );
    let keychain = ControlledKeychain::new(SharedKeychain::new());
    keychain.locked.store(true, Ordering::SeqCst);
    let engine = PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        Some(vauchi_platform::HandedOverSecret {
            handle: "legacy-storage-key".into(),
            secret: shell_key.as_bytes().to_vec(),
        }),
        Box::new(keychain.clone()),
    )
    .unwrap();

    let locked = parse(engine.initial_commands_json().unwrap());
    assert_eq!(released(&locked), Vec::<String>::new());

    keychain.locked.store(false, Ordering::SeqCst);
    let opened = prompt_succeeded(&engine);

    assert_eq!(released(&opened), ["legacy-storage-key"]);
    assert_eq!(keychain.store.names(), ["smk"]);
}
