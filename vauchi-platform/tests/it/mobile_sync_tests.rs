// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the engine-resident sync surface — `DomainCommand::Sync`.
//!
//! Migrates the legacy `VauchiPlatform::sync()` onto `PlatformAppEngine`
//! per the Phase-2b sync-orchestration design
//! (`_private/docs/designs/2026-06-09-engine-resident-sync-orchestration-design.md`,
//! collapse-vauchi-platform G1). The persistent engine now owns the
//! connect lifecycle; user-initiated sync honors the C1/C2 timing
//! throttle, so a `TooSoon` outcome is a benign no-change result, not an
//! error.
//!
//! The relay-connected happy path (received/sent counts over the wire)
//! stays e2e-covered; these tests cover the deterministic contract: the
//! connect-lifecycle gate (no identity) and the `VauchiSyncOutcome` →
//! `MobileSyncResult` policy mapping (including the throttle decision).

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_core::api::VauchiSyncOutcome;
use vauchi_platform::{DomainCommand, MobileSyncResult, PlatformAppEngine};

fn fresh_engine() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = TempDir::new().unwrap();
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
        key.as_bytes().to_vec(),
    )
    .expect("create PlatformAppEngine");
    (engine, dir)
}

// @internal
#[test]
fn sync_without_identity_surfaces_identity_error() {
    // A fresh engine has no identity. The handler's lazy connect()
    // (no cached OHTTP key) hits the identity gate before any network,
    // so the contract is deterministic and relay-free.
    let (engine, _dir) = fresh_engine();

    let err = engine
        .dispatch_domain_command(DomainCommand::Sync)
        .expect_err("sync without an identity must error, not return a result");

    let detail = format!("{err:?}").to_lowercase();
    assert!(
        detail.contains("identity"),
        "no-identity sync must surface an identity error; got: {detail}"
    );
}

// @internal
#[test]
fn too_soon_maps_to_benign_no_change_result() {
    // The throttle decision (design §4): a C1/C2 deferral is NOT an
    // error — it is an up-to-date / no-change result.
    let result = MobileSyncResult::try_from(VauchiSyncOutcome::TooSoon)
        .expect("TooSoon must map to a result, not an error");

    assert!(!result.has_changes, "TooSoon must report no changes");
    assert_eq!(result.total, 0, "TooSoon must report zero total operations");
    assert_eq!(result.cards_updated, 0, "TooSoon: no cards updated");
    assert_eq!(result.updates_sent, 0, "TooSoon: no updates sent");
    assert_eq!(result.contacts_added, 0, "TooSoon: no contacts added");
    assert!(
        result.updated_contact_names.is_empty(),
        "TooSoon: no updated names"
    );
}

// @internal
#[test]
fn ok_outcome_maps_received_and_sent_counts() {
    let outcome = VauchiSyncOutcome::Ok {
        received: 3,
        sent: 2,
        acknowledged: 0,
        errors: vec![],
        version_policy: None,
    };

    let result = MobileSyncResult::try_from(outcome).expect("Ok outcome must map to a result");

    assert_eq!(result.cards_updated, 3, "received maps to cards_updated");
    assert_eq!(result.updates_sent, 2, "sent maps to updates_sent");
    assert_eq!(result.total, 5, "total is received + sent");
    assert!(result.has_changes, "received>0 || sent>0 means has_changes");
    assert_eq!(
        result.contacts_added, 0,
        "contacts_added stays 0 (outcome carries no such count, matching legacy)"
    );
}

// @internal
#[test]
fn ok_outcome_with_zero_counts_has_no_changes() {
    let outcome = VauchiSyncOutcome::Ok {
        received: 0,
        sent: 0,
        acknowledged: 0,
        errors: vec![],
        version_policy: None,
    };

    let result = MobileSyncResult::try_from(outcome).expect("Ok outcome must map to a result");

    assert!(
        !result.has_changes,
        "an empty Ok sync must report no changes"
    );
    assert_eq!(result.total, 0, "empty Ok sync has zero total");
}

// @internal
#[test]
fn not_connected_outcome_maps_to_error() {
    let err = MobileSyncResult::try_from(VauchiSyncOutcome::NotConnected)
        .expect_err("NotConnected must map to an error, not a silent zero result");
    let detail = format!("{err:?}").to_lowercase();
    assert!(
        detail.contains("connect"),
        "NotConnected error must mention connection; got: {detail}"
    );
}

// @internal
#[test]
fn no_identity_outcome_maps_to_error() {
    let err = MobileSyncResult::try_from(VauchiSyncOutcome::NoIdentity)
        .expect_err("NoIdentity must map to an error, not a silent zero result");
    let detail = format!("{err:?}").to_lowercase();
    assert!(
        detail.contains("identity"),
        "NoIdentity error must mention identity; got: {detail}"
    );
}
