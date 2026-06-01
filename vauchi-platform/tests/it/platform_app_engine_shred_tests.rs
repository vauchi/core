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

fn engine_with_identity() -> (Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("create_new");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .expect("display_name");
    for _ in 0..3 {
        engine
            .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
            .expect("continue");
    }
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start_app"}}"#.into())
        .expect("start_app");
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
