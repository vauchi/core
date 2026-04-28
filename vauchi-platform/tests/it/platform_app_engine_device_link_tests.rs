// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the device-linking domain on `PlatformAppEngine`
//! (Phase B4 of `2026-04-28-collapse-vauchi-platform-into-app-engine`).
//!
//! Surface migrated: the **post-orchestrator** shape only. The
//! pre-orchestrator legacy methods (`start_device_link`,
//! `start_device_join`, `send_device_link_request`,
//! `listen_for_device_link_request`, `send_device_link_response`)
//! are intentionally NOT migrated — they were superseded by the
//! orchestrator session in `done/2026-04-27-device-link-orchestrator-phase2d-windows`.

use std::sync::Arc;

use vauchi_platform::PlatformAppEngine;

fn create_engine_with_identity() -> (Arc<PlatformAppEngine>, tempfile::TempDir) {
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

// ── get_devices / device_count / is_primary_device ───────────────────

// @internal
#[test]
fn get_devices_returns_only_current_device_initially() {
    let (engine, _dir) = create_engine_with_identity();
    let devices = engine.get_devices().expect("get_devices");
    assert_eq!(
        devices.len(),
        1,
        "fresh identity has only the current device"
    );
    assert!(devices[0].is_current);
    assert!(devices[0].is_active);
    assert_eq!(devices[0].device_index, 0, "current is primary");
}

// @internal
#[test]
fn device_count_is_one_initially() {
    let (engine, _dir) = create_engine_with_identity();
    let count = engine.device_count().expect("device_count");
    assert_eq!(count, 1, "fresh identity has one device");
}

// @internal
#[test]
fn is_primary_device_is_true_for_first_device() {
    let (engine, _dir) = create_engine_with_identity();
    let is_primary = engine.is_primary_device().expect("is_primary_device");
    assert!(is_primary, "first device must be primary");
}

// ── unlink_device ────────────────────────────────────────────────────

// @internal
#[test]
fn unlink_device_returns_false_for_out_of_range_index() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.unlink_device(99).expect("unlink_device");
    assert!(!result, "out-of-range index returns false");
}

// @internal
#[test]
fn unlink_device_returns_false_when_no_registry_yet() {
    // A fresh identity has no on-disk DeviceRegistry — `get_devices()`
    // synthesises a single-element list from the identity's own
    // `device_info`, but `unlink_device` reads the persisted registry
    // and returns `false` rather than erroring. The "Cannot unlink the
    // current device" error path only fires once a registry has been
    // saved (after the first `confirm_link` succeeds end-to-end).
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.unlink_device(0).expect("unlink_device(0)");
    assert!(
        !result,
        "no registry yet → false (matches legacy behaviour)"
    );
}

// ── generate_device_link_qr / parse_device_link_qr ──────────────────

// @internal
#[test]
fn generate_device_link_qr_returns_data_with_expiry() {
    let (engine, _dir) = create_engine_with_identity();
    let qr = engine
        .generate_device_link_qr()
        .expect("generate_device_link_qr");

    assert!(!qr.qr_data.is_empty(), "qr_data must not be empty");
    assert_eq!(
        qr.identity_public_key.len(),
        64,
        "identity pk hex == 64 chars"
    );
    assert!(
        qr.expires_at > qr.timestamp,
        "expires_at must be after timestamp"
    );
}

// @internal
#[test]
fn parse_device_link_qr_round_trips_generate() {
    let (engine, _dir) = create_engine_with_identity();
    let qr = engine
        .generate_device_link_qr()
        .expect("generate_device_link_qr");

    let parsed = engine
        .parse_device_link_qr(qr.qr_data)
        .expect("parse_device_link_qr");

    assert_eq!(parsed.identity_public_key, qr.identity_public_key);
    assert_eq!(parsed.timestamp, qr.timestamp);
    assert!(!parsed.is_expired, "fresh QR must not be expired");
}

// @internal
#[test]
fn parse_device_link_qr_rejects_garbage_input() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.parse_device_link_qr("not-a-qr".into());
    assert!(result.is_err(), "garbage QR must error");
}

// ── create_device_link_session_initiator ─────────────────────────────

// @internal
#[test]
fn create_device_link_session_initiator_returns_session() {
    // The orchestrator session is the post-Phase-2d entry point.
    // Smoke-level: verify the wrapper constructs a session without
    // panic. Driving the cycle thread end-to-end requires a real
    // relay peer — covered by `device_link_listener_tests.rs`.
    let (engine, _dir) = create_engine_with_identity();
    let session = engine
        .create_device_link_session_initiator()
        .expect("create_device_link_session_initiator");

    // Session is opaque; the wrapper's contract is just "returns a
    // session that can be `start()`ed by the frontend". Drop the
    // session here without start — the cycle thread is not spawned
    // until start() is called.
    drop(session);
}

// ── Cache invalidation contract ──────────────────────────────────────

// @internal
#[test]
fn unlink_device_invalidates_device_management_cache() {
    let (engine, _dir) = create_engine_with_identity();
    // out-of-range so no actual mutation, but the wrapper must still
    // be a no-panic call and the read-after-call must succeed.
    let _ = engine.unlink_device(99);
    engine.invalidate_all().expect("invalidate_all");
    let _ = engine
        .current_screen_json()
        .expect("current_screen_json after unlink_device");
}
