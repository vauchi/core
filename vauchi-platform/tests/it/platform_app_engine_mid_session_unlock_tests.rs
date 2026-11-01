// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A keychain whose authentication lapsed in the middle of a session
//! (Android release builds allow Keystore use for 300 s after an unlock)
//! makes Core ask for the unlock prompt and retry the action once it
//! succeeds, instead of failing the action (vauchi/private#1021, owner
//! decision 2026-11-01: ask again, then retry).
//!
//! Traces to: features/storage_key_upgrade.feature

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use vauchi_platform::{KeychainError, MobilePlatformKeychain, PlatformAppEngine};

use crate::support::SharedKeychain;

const RELAY: &str = "https://relay.test";

/// A keychain whose writes need an unlock while `saves_locked` is set.
#[derive(Clone)]
struct LapsingKeychain {
    store: SharedKeychain,
    saves_locked: Arc<AtomicBool>,
}

impl MobilePlatformKeychain for LapsingKeychain {
    fn save_key(&self, name: String, key: Vec<u8>) -> Result<(), KeychainError> {
        if self.saves_locked.load(Ordering::SeqCst) {
            return Err(KeychainError::AuthenticationRequired);
        }
        self.store.save_key(name, key)
    }
    fn load_key(&self, name: String) -> Result<Option<Vec<u8>>, KeychainError> {
        self.store.load_key(name)
    }
    fn delete_key(&self, name: String) -> Result<(), KeychainError> {
        self.store.delete_key(name)
    }
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

fn enter_name(engine: &PlatformAppEngine, batch: &Value, name: &str) -> Value {
    let surface = batch["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("ReplaceSurface"))
        .expect("a surface")["surface"]
        .clone();
    let binding = surface["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|node| node.get("Input"))
        .expect("the name input")["binding_id"]
        .clone();
    let event = serde_json::json!({
        "ValueChanged": {
            "surface_id": surface["surface_id"],
            "binding_id": binding,
            "value": { "text": name },
        }
    });
    parse(engine.dispatch_json(event.to_string()).expect("dispatch"))
}

fn prompt_succeeded(engine: &PlatformAppEngine) -> Value {
    let event = serde_json::to_string(&vauchi_core::Event::BiometricUnlockSucceeded).unwrap();
    parse(engine.dispatch_json(event).expect("dispatch"))
}

/// A fresh install on the last onboarding step, with the keychain's
/// authentication lapsed: the next press creates the identity.
fn on_the_last_step_with_the_unlock_lapsed() -> (
    tempfile::TempDir,
    LapsingKeychain,
    Arc<PlatformAppEngine>,
    Value,
) {
    let dir = tempfile::tempdir().unwrap();
    let keychain = LapsingKeychain {
        store: SharedKeychain::new(),
        saves_locked: Arc::new(AtomicBool::new(false)),
    };
    let engine = PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        None,
        Box::new(keychain.clone()),
    )
    .expect("engine");
    let mut batch = parse(engine.initial_commands_json().unwrap());
    batch = press_primary(&engine, &batch); // identity_check → default_name
    batch = enter_name(&engine, &batch, "Alice");
    batch = press_primary(&engine, &batch); // default_name → groups_setup
    batch = press_primary(&engine, &batch); // groups_setup → contact_info
    batch = press_primary(&engine, &batch); // contact_info → what_next
    keychain.saves_locked.store(true, Ordering::SeqCst);
    (dir, keychain, engine, batch)
}

// @internal
#[test]
fn a_lapsed_unlock_during_onboarding_asks_for_the_prompt_instead_of_failing() {
    let (_dir, _keychain, engine, batch) = on_the_last_step_with_the_unlock_lapsed();

    let after = press_primary(&engine, &batch);

    let names = command_names(&after);
    assert!(
        names.contains(&"RequestBiometricUnlock".to_string()),
        "a lapsed keychain must ask for the unlock, got {names:?}"
    );
    assert!(
        !names.contains(&"PresentAlert".to_string()),
        "a lapsed keychain must not fail the step with an alert, got {names:?}"
    );
    assert!(
        !engine.has_identity().unwrap(),
        "nothing is created before the unlock"
    );
}

// @internal
#[test]
fn the_unlock_retries_the_action_the_lapsed_keychain_held_up() {
    let (_dir, keychain, engine, batch) = on_the_last_step_with_the_unlock_lapsed();
    press_primary(&engine, &batch);
    keychain.saves_locked.store(false, Ordering::SeqCst);

    let after = prompt_succeeded(&engine);

    assert!(
        engine.has_identity().unwrap(),
        "the unlock must create the identity"
    );
    let surface = command_names(&after)
        .iter()
        .position(|name| name == "ReplaceSurface")
        .map(|index| after["commands"][index]["ReplaceSurface"]["surface"].to_string())
        .expect("the unlock must show the created card");
    assert!(
        surface.contains("\"Alice\""),
        "the card must carry the entered name"
    );
}

// @internal
#[test]
fn an_unlock_with_no_held_action_changes_nothing() {
    let (_dir, keychain, engine, _batch) = on_the_last_step_with_the_unlock_lapsed();
    keychain.saves_locked.store(false, Ordering::SeqCst);

    let after = prompt_succeeded(&engine);

    assert!(!engine.has_identity().unwrap());
    assert!(
        !command_names(&after).contains(&"RequestBiometricUnlock".to_string()),
        "a stray unlock must not ask again"
    );
}
