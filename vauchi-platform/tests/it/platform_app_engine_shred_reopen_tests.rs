// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! After a shred the person continues on a fresh install
//! (vauchi/private#599). The shred deletes the database the open engine
//! holds; without reopening, a new identity is written into the deleted
//! file and is gone at the next start.

use std::sync::Arc;

use vauchi_platform::{DomainCommand, DomainCommandResult, PlatformAppEngine};

use crate::support::{SharedKeychain, drive_onboarding};

const RELAY: &str = "https://relay.test";

fn open(dir: &tempfile::TempDir, keychain: &SharedKeychain) -> Arc<PlatformAppEngine> {
    PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        None,
        Box::new(keychain.clone()),
    )
    .expect("open")
}

// @internal
#[test]
fn after_a_panic_shred_the_next_identity_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    drive_onboarding(&engine);
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));
    assert!(
        !engine.has_identity().unwrap(),
        "the shred left the identity"
    );

    drive_onboarding(&engine);
    assert!(engine.has_identity().unwrap());
    drop(engine);

    let restarted = open(&dir, &keychain);
    assert!(
        restarted.has_identity().unwrap(),
        "the identity created after the shred did not survive a restart"
    );
}

fn current_surface(engine: &PlatformAppEngine) -> String {
    let batch: serde_json::Value =
        serde_json::from_str(&engine.initial_commands_json().expect("initial commands"))
            .expect("parse command batch");
    batch["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
        .and_then(|bar| bar["surface_id"].as_str())
        .expect("command batch carries a context bar")
        .to_owned()
}

/// A shell can still report on the screen a shred left: iOS ends the focus
/// of the confirmation field when Welcome replaces it. The reopened engine
/// never showed that screen, and must drop the late event as the shredded
/// engine would have, not fail it (vauchi/private#293).
// @internal
#[test]
fn a_late_event_for_the_screen_before_a_shred_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    drive_onboarding(&engine);
    let before_shred = current_surface(&engine);
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));
    let fresh = current_surface(&engine);
    assert_ne!(
        fresh, before_shred,
        "precondition: the reopen shows another screen"
    );

    let late = serde_json::json!({
        "InputFocusEnded": { "surface_id": before_shred, "binding_id": "confirm" }
    });
    let reply = engine.dispatch_json(late.to_string());

    let batch: serde_json::Value =
        serde_json::from_str(&reply.expect("a late event is dropped, not an error"))
            .expect("parse command batch");
    assert_eq!(batch["commands"], serde_json::json!([]));
    assert_eq!(
        current_surface(&engine),
        fresh,
        "the late event moved the screen"
    );
}
