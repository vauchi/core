// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lock-contention regression test for the single-engine collapse
//! (`2026-04-28-collapse-vauchi-platform-into-app-engine` G6).
//!
//! The original bug: iOS/Android held *two* Rust instances over the same
//! SQLite file (`VauchiPlatform` + `PlatformAppEngine`), so concurrent
//! write paths (sync + per-field write + delivery-retry) raced for the
//! database lock and surfaced transient `database is locked` errors. The
//! collapse left a single `PlatformAppEngine` handle whose `AppEngine`
//! mutex serialises every operation onto one storage connection.
//!
//! This test exercises the three historically-contended paths
//! simultaneously through that single handle and asserts none of them
//! errors with a lock-contention failure (and that the engine is still
//! responsive afterwards). It guards against any future reintroduction
//! of a second handle to the same DB.

use std::sync::Arc;
use std::thread;

use tempfile::TempDir;

use vauchi_core::storage::{DeliveryRecord, DeliveryStatus};
use vauchi_platform::{
    DomainCommand, DomainCommandResult, PlatformAppEngine, PlatformAppEngineTestHelpers,
};

fn setup() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = TempDir::new().unwrap();
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
        key.as_bytes().to_vec(),
    )
    .expect("create PlatformAppEngine");
    drive_onboarding(&engine);
    (engine, dir)
}

fn drive_onboarding(engine: &PlatformAppEngine) {
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
}

fn make_record(message_id: &str) -> DeliveryRecord {
    let ts = vauchi_core::clock::SystemClock::shared().unix_seconds();
    DeliveryRecord {
        message_id: message_id.to_string(),
        recipient_id: "contact-1".to_string(),
        status: DeliveryStatus::Failed {
            reason: "network".to_string(),
        },
        created_at: ts,
        updated_at: ts,
        expires_at: None,
    }
}

/// `true` if the error reads like SQLite lock contention — the exact
/// failure mode the collapse was meant to eliminate.
fn is_lock_contention(detail: &str) -> bool {
    let d = detail.to_lowercase();
    d.contains("database is locked") || d.contains("database table is locked") || d.contains("busy")
}

// @scenario: device_sync:Concurrent sync + write + delivery has no lock contention
// @internal
#[test]
fn concurrent_sync_write_delivery_has_no_lock_contention() {
    let (engine, _dir) = setup();

    const THREADS_PER_ROLE: usize = 4;
    const ITERS: usize = 40;

    let mut handles = Vec::new();

    // Role 1 — sync. periodic_sync_tick gate-reads storage every call;
    // no identity-less / no-relay path touches the network, so it
    // exercises the read side of the lock without external deps.
    for _ in 0..THREADS_PER_ROLE {
        let e = Arc::clone(&engine);
        handles.push(thread::spawn(move || -> Vec<String> {
            let mut errs = Vec::new();
            for _ in 0..ITERS {
                if let Err(err) = e.periodic_sync_tick() {
                    errs.push(format!("{err:?}"));
                }
            }
            errs
        }));
    }

    // Role 2 — per-field write (own-card display name mutation).
    for t in 0..THREADS_PER_ROLE {
        let e = Arc::clone(&engine);
        handles.push(thread::spawn(move || -> Vec<String> {
            let mut errs = Vec::new();
            for i in 0..ITERS {
                let cmd = DomainCommand::SetDisplayName {
                    name: format!("Alice-{t}-{i}"),
                };
                if let Err(err) = e.dispatch_domain_command(cmd) {
                    errs.push(format!("{err:?}"));
                }
            }
            errs
        }));
    }

    // Role 3 — delivery-retry: write a delivery record + read failed set.
    for t in 0..THREADS_PER_ROLE {
        let e = Arc::clone(&engine);
        handles.push(thread::spawn(move || -> Vec<String> {
            let mut errs = Vec::new();
            for i in 0..ITERS {
                if let Err(err) = e.save_test_delivery_record(&make_record(&format!("msg-{t}-{i}")))
                {
                    errs.push(format!("{err:?}"));
                }
                if let Err(err) = e.dispatch_domain_command(DomainCommand::GetFailedDeliveryRecords)
                {
                    errs.push(format!("{err:?}"));
                }
            }
            errs
        }));
    }

    let all_errs: Vec<String> = handles
        .into_iter()
        .flat_map(|h| h.join().expect("worker thread must not panic"))
        .collect();

    let lock_errs: Vec<&String> = all_errs.iter().filter(|e| is_lock_contention(e)).collect();
    assert!(
        lock_errs.is_empty(),
        "single-handle concurrent sync+write+delivery must not hit SQLite \
         lock contention; got {} lock error(s): {lock_errs:?}",
        lock_errs.len()
    );

    // The engine must still be responsive after the concurrent storm.
    match engine
        .dispatch_domain_command(DomainCommand::GetAllDeliveryRecords)
        .expect("engine still responds after concurrent load")
    {
        DomainCommandResult::DeliveryRecords { records } => assert!(
            records.len() >= THREADS_PER_ROLE * ITERS,
            "all written delivery records must be readable (got {})",
            records.len()
        ),
        other => panic!("expected DeliveryRecords, got {other:?}"),
    }
}
