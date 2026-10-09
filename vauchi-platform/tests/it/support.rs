// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared arrange helpers for the platform integration tests: an in-memory
//! platform keychain and onboarding driven through the generic envelope.

use std::collections::HashMap;
use std::sync::Mutex;

use vauchi_platform::{KeychainError, MobilePlatformKeychain, PlatformAppEngine};

pub struct FakeKeychain {
    pub store: Mutex<HashMap<String, Vec<u8>>>,
}

impl FakeKeychain {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl MobilePlatformKeychain for FakeKeychain {
    fn save_key(&self, name: String, key: Vec<u8>) -> Result<(), KeychainError> {
        self.store.lock().unwrap().insert(name, key);
        Ok(())
    }
    fn load_key(&self, name: String) -> Result<Option<Vec<u8>>, KeychainError> {
        Ok(self.store.lock().unwrap().get(&name).cloned())
    }
    fn delete_key(&self, name: String) -> Result<(), KeychainError> {
        self.store.lock().unwrap().remove(&name);
        Ok(())
    }
}

/// Drive through the full onboarding flow via the canonical envelope.
///
/// Every step reads the Core-minted interaction and binding ids from the
/// current command batch — exactly what a real shell renders — and
/// dispatches generic events back. No retired action/screen seams.
pub fn drive_onboarding(engine: &PlatformAppEngine) {
    fn primary_interaction(batch: &serde_json::Value) -> (String, String) {
        let bar = batch["commands"]
            .as_array()
            .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
            .expect("command batch must carry a context bar");
        (
            bar["surface_id"]
                .as_str()
                .expect("bar surface id")
                .to_owned(),
            bar["bar"]["primary"]["interaction_id"]
                .as_str()
                .expect("primary interaction id")
                .to_owned(),
        )
    }

    fn dispatch_primary(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
    ) -> serde_json::Value {
        let (surface_id, interaction_id) = primary_interaction(batch);
        let event = serde_json::json!({
            "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch primary activation"),
        )
        .expect("parse command batch")
    }

    fn find_input(nodes: &[serde_json::Value]) -> Option<&serde_json::Value> {
        nodes.iter().find_map(|node| {
            if let Some(input) = node.get("Input") {
                Some(input)
            } else {
                node["Group"]["children"]
                    .as_array()
                    .and_then(|children| find_input(children))
            }
        })
    }

    fn set_text_input(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
        text: &str,
    ) -> serde_json::Value {
        let (surface_id, nodes) = batch["commands"]
            .as_array()
            .and_then(|commands| {
                commands.iter().find_map(|c| {
                    let surface = &c["ReplaceSurface"]["surface"];
                    surface
                        .is_object()
                        .then(|| (surface["surface_id"].clone(), surface["nodes"].clone()))
                })
            })
            .expect("command batch must replace a surface");
        let nodes: Vec<serde_json::Value> =
            serde_json::from_value(nodes).expect("surface nodes array");
        let input = find_input(&nodes).expect("surface must carry a text input");
        let event = serde_json::json!({
            "ValueChanged": {
                "surface_id": surface_id,
                "binding_id": input["binding_id"],
                "value": { "text": text },
            }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch text input"),
        )
        .expect("parse command batch")
    }

    let mut batch: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("initial onboarding commands"),
    )
    .expect("parse initial batch");

    batch = dispatch_primary(engine, &batch); // identity_check → default_name
    batch = set_text_input(engine, &batch, "Alice"); // enter display name
    batch = dispatch_primary(engine, &batch); // default_name → groups_setup
    batch = dispatch_primary(engine, &batch); // groups_setup → contact_info
    batch = dispatch_primary(engine, &batch); // contact_info → what_next
    let _ = dispatch_primary(engine, &batch); // what_next → complete → home
}

/// A handle to a [`FakeKeychain`] the test keeps reading after handing it
/// to Core, which takes the keychain by value.
#[derive(Clone)]
pub struct SharedKeychain(pub std::sync::Arc<FakeKeychain>);

impl SharedKeychain {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(FakeKeychain::new()))
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.0.store.lock().unwrap().keys().cloned().collect();
        names.sort();
        names
    }
}

impl MobilePlatformKeychain for SharedKeychain {
    fn save_key(&self, name: String, key: Vec<u8>) -> Result<(), KeychainError> {
        self.0.save_key(name, key)
    }
    fn load_key(&self, name: String) -> Result<Option<Vec<u8>>, KeychainError> {
        self.0.load_key(name)
    }
    fn delete_key(&self, name: String) -> Result<(), KeychainError> {
        self.0.delete_key(name)
    }
}

/// The sibling `locales` checkout. Prefers `VAUCHI_LOCALES_DIR` (exported by
/// CI and `just mutants-diff`) because cargo-mutants builds a temp copy where
/// the relative sibling does not exist.
pub fn locales_dir() -> std::path::PathBuf {
    std::env::var_os("VAUCHI_LOCALES_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales")
        })
}
