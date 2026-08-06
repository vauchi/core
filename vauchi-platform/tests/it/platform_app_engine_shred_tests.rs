// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the B7 keychain-bound shred `DomainCommand`s
//! (`SoftShred` / `CancelShred`) on `PlatformAppEngine` — Phase 1a.
//!
//! The shred crypto runs through the real `vauchi_core::api::ShredManager`
//! (ADR-002, no mocking). The keychain is *platform secure storage*
//! (iOS Keychain / Android KeyStore), so an in-memory fake is the correct
//! test double — `soft_shred` / `cancel_shred` only touch the deletion
//! schedule in storage, not the SMK, so the store can be empty.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vauchi_platform::{
    DomainCommand, DomainCommandResult, KeychainError, MobilePlatformKeychain, MobileShredStatus,
    PlatformAppEngine,
};

struct FakeKeychain {
    store: Mutex<HashMap<String, Vec<u8>>>,
}

impl FakeKeychain {
    fn new() -> Self {
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
fn drive_onboarding(engine: &PlatformAppEngine) {
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

fn engine_with_identity() -> (Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    drive_onboarding(&engine);
    (engine, dir)
}

// @internal
#[test]
fn soft_shred_without_keychain_errors() {
    let (engine, _dir) = engine_with_identity();
    // No keychain set → the dispatch arm's bridge build must fail.
    let result = engine.dispatch_domain_command(DomainCommand::SoftShred);
    assert!(
        result.is_err(),
        "SoftShred without a platform keychain must error, got {result:?}"
    );
}

// @internal
#[test]
fn soft_shred_schedules_then_cancel_clears() {
    let (engine, _dir) = engine_with_identity();
    engine.set_platform_keychain(Box::new(FakeKeychain::new()));

    // SoftShred → ShredScheduled carrying a token.
    let token = match engine
        .dispatch_domain_command(DomainCommand::SoftShred)
        .expect("soft_shred dispatch")
    {
        DomainCommandResult::ShredScheduled { token } => token,
        other => panic!("expected ShredScheduled, got {other:?}"),
    };

    // Status now reflects a scheduled shred.
    match engine
        .dispatch_domain_command(DomainCommand::ShredStatus)
        .expect("status dispatch")
    {
        DomainCommandResult::ShredStatus { status } => assert!(
            matches!(status, MobileShredStatus::Scheduled { .. }),
            "expected Scheduled after soft_shred, got {status:?}"
        ),
        other => panic!("expected ShredStatus, got {other:?}"),
    }

    // CancelShred with the token → Unit, status back to None.
    match engine
        .dispatch_domain_command(DomainCommand::CancelShred { token })
        .expect("cancel_shred dispatch")
    {
        DomainCommandResult::Unit => {}
        other => panic!("expected Unit, got {other:?}"),
    }
    match engine
        .dispatch_domain_command(DomainCommand::ShredStatus)
        .expect("status dispatch 2")
    {
        DomainCommandResult::ShredStatus { status } => assert!(
            matches!(status, MobileShredStatus::None),
            "expected None after cancel_shred, got {status:?}"
        ),
        other => panic!("expected ShredStatus, got {other:?}"),
    }
}

// @internal
#[test]
fn panic_shred_returns_report_and_destroys_storage() {
    let (engine, _dir) = engine_with_identity();
    engine.set_platform_keychain(Box::new(FakeKeychain::new()));

    // PanicShred is immediate + irreversible. The relay purge/revocation
    // can't reach the non-resolving test relay URL, but local destruction
    // (real ShredManager, ADR-002) still completes.
    let report = match engine
        .dispatch_domain_command(DomainCommand::PanicShred)
        .expect("panic_shred dispatch")
    {
        DomainCommandResult::ShredCompleted { report } => report,
        other => panic!("expected ShredCompleted, got {other:?}"),
    };
    assert!(
        report.sqlite_destroyed,
        "panic_shred must destroy the database, got {report:?}"
    );
}

// @internal
#[test]
fn hard_shred_before_grace_period_errors() {
    let (engine, _dir) = engine_with_identity();
    engine.set_platform_keychain(Box::new(FakeKeychain::new()));

    let token = match engine
        .dispatch_domain_command(DomainCommand::SoftShred)
        .expect("soft_shred dispatch")
    {
        DomainCommandResult::ShredScheduled { token } => token,
        other => panic!("expected ShredScheduled, got {other:?}"),
    };
    // Immediate hard-shred → grace period has not elapsed → error.
    let result = engine.dispatch_domain_command(DomainCommand::HardShred { token });
    assert!(
        result.is_err(),
        "HardShred before the grace period must error, got {result:?}"
    );
}
