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
