// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the Pair 5 `DeviceLinkEngineBridge` listener
//! wiring on `PlatformAppEngine`. Pair 5 of
//! `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`.
//!
//! Coverage:
//! - Lifecycle: navigating to `DeviceLinking` ensures a session;
//!   navigating away cancels it.
//! - Bridge forwarding: simulated cycle-thread events drive the
//!   engine's receiver-side state via the test-only
//!   `apply_device_link_*_for_test` helpers.
//! - Action interception: the engine's typed device-link
//!   `ActionResult` variants surface in `handle_action_json` so the
//!   frontend never has to call session methods directly.

use std::sync::Arc;

use vauchi_platform::{PlatformAppEngine, PlatformAppEngineTestHelpers};

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

fn navigate_to_device_linking(engine: &PlatformAppEngine) {
    engine
        .navigate_to_json(r#""DeviceLinking""#.into())
        .expect("navigate_to DeviceLinking");
}

/// Bridge-forwarding tests use this to stop the live cycle thread
/// (which would otherwise overwrite test-driven state via `on_qr_ready`
/// or `on_failed`). Lifecycle tests skip it because they want to
/// observe the production session-presence behaviour.
fn navigate_to_device_linking_quiescent(engine: &PlatformAppEngine) {
    navigate_to_device_linking(engine);
    engine.cancel_device_link_session_for_test();
}

fn current_screen_id(engine: &PlatformAppEngine) -> String {
    let json = engine.current_screen_json().expect("current_screen_json");
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse screen json");
    v.get("screen_id")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string()
}

// ── Lifecycle ──────────────────────────────────────────────────

// @scenario: pair5_device_link_listener :: navigation into DeviceLinking spawns a session
#[test]
fn navigating_to_device_linking_ensures_session() {
    let (engine, _dir) = create_engine_with_identity();
    assert!(!engine.device_link_session_is_active_for_test());
    navigate_to_device_linking(&engine);
    assert!(
        engine.device_link_session_is_active_for_test(),
        "session not held after navigating to DeviceLinking"
    );
}

// @scenario: pair5_device_link_listener :: ensure_device_link_session is idempotent
#[test]
fn re_navigating_to_device_linking_keeps_single_session() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking(&engine);
    let _ = engine.navigate_to_json(r#""Settings""#.into());
    assert!(!engine.device_link_session_is_active_for_test());
    navigate_to_device_linking(&engine);
    assert!(engine.device_link_session_is_active_for_test());
    // Re-navigate without leaving — still exactly one session.
    navigate_to_device_linking(&engine);
    assert!(engine.device_link_session_is_active_for_test());
}

// @scenario: pair5_device_link_listener :: navigating away cancels the session
#[test]
fn navigating_away_from_device_linking_cancels_session() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking(&engine);
    assert!(engine.device_link_session_is_active_for_test());
    let _ = engine.navigate_to_json(r#""Settings""#.into());
    assert!(
        !engine.device_link_session_is_active_for_test(),
        "session not cancelled after leaving DeviceLinking"
    );
}

// ── Bridge forwarding ──────────────────────────────────────────

// @scenario: pair5_device_link_listener :: on_qr_ready advances the engine to waiting
#[test]
fn qr_ready_bridge_advances_engine_to_waiting() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_qr_ready_for_test("test-qr".into(), 1_700_000_500)
        .expect("apply qr_ready");
    assert_eq!(current_screen_id(&engine), "link_waiting");
}

// @scenario: pair5_device_link_listener :: on_confirmation_required advances the engine
#[test]
fn request_received_bridge_advances_engine_to_confirming_device() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_request_received_for_test(
            "New iPad".into(),
            "112233".into(),
            "deadbeef".into(),
        )
        .expect("apply request_received");
    assert_eq!(current_screen_id(&engine), "link_confirming_device");
}

// @scenario: pair5_device_link_listener :: on_failed("qr_expired") routes to the expired screen
#[test]
fn qr_expired_bridge_routes_to_qr_expired_screen() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_qr_expired_for_test()
        .expect("apply qr_expired");
    assert_eq!(current_screen_id(&engine), "link_qr_expired");
}

// @scenario: pair5_device_link_listener :: on_failed(reason) routes to the failed screen
#[test]
fn failed_bridge_routes_to_failed_screen_with_reason() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_failed_for_test("relay timeout".into())
        .expect("apply failed");
    assert_eq!(current_screen_id(&engine), "link_failed");
}

// @scenario: pair5_device_link_listener :: on_completed routes to the terminal Complete screen
#[test]
fn completed_bridge_routes_to_complete_screen() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_completed_for_test()
        .expect("apply completed");
    assert_eq!(current_screen_id(&engine), "link_complete");
}

// ── Action interception ───────────────────────────────────────

// @scenario: pair5_device_link_listener :: confirm_manual surfaces DeviceLinkConfirmManual JSON
#[test]
fn confirm_manual_action_emits_typed_action_result_json() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_request_received_for_test(
            "New iPad".into(),
            "654321".into(),
            "deadbeef".into(),
        )
        .expect("apply request_received");
    let _ = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "codes_match"}}"#.into())
        .expect("codes_match");
    let result_json = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "confirm_manual"}}"#.into())
        .expect("confirm_manual");
    let parsed: serde_json::Value =
        serde_json::from_str(&result_json).expect("parse action result");
    let confirm = parsed
        .get("action_result")
        .and_then(|r| r.get("DeviceLinkConfirmManual"))
        .expect("DeviceLinkConfirmManual variant");
    assert_eq!(confirm.get("code").and_then(|c| c.as_str()), Some("654321"));
    // Engine has advanced to Completing.
    assert_eq!(current_screen_id(&engine), "link_completing");
}

// @scenario: pair5_device_link_listener :: deny surfaces DeviceLinkDeny JSON
#[test]
fn deny_action_emits_device_link_deny_json() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_request_received_for_test(
            "New iPad".into(),
            "654321".into(),
            "deadbeef".into(),
        )
        .expect("apply request_received");
    let result_json = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "deny"}}"#.into())
        .expect("deny");
    let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("parse envelope");
    assert_eq!(parsed["action_result"], serde_json::json!("DeviceLinkDeny"));
}

// @scenario: pair5_device_link_listener :: retry from QrExpired emits DeviceLinkRetry and rotates session
#[test]
fn retry_from_expired_emits_device_link_retry_and_rotates_session() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_qr_expired_for_test()
        .expect("apply qr_expired");
    assert_eq!(current_screen_id(&engine), "link_qr_expired");

    let result_json = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "retry"}}"#.into())
        .expect("retry");
    let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("parse envelope");
    assert_eq!(
        parsed["action_result"],
        serde_json::json!("DeviceLinkRetry")
    );
    // Session rotated — a fresh one is held.
    assert!(engine.device_link_session_is_active_for_test());
}

// @scenario: pair5_device_link_listener :: cancel from device-link screen leaves no active session
#[test]
fn cancel_from_device_link_screen_drops_session() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking(&engine);
    assert!(
        engine.device_link_session_is_active_for_test(),
        "session not held after navigating in"
    );

    let _ = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "cancel"}}"#.into())
        .expect("cancel");
    // route_result(Complete) → handle_completion → navigate away;
    // after_screen_transition then drops the session.
    assert!(
        !engine.device_link_session_is_active_for_test(),
        "session not dropped after cancel"
    );
    assert_ne!(current_screen_id(&engine), "link_confirming_device");
}
