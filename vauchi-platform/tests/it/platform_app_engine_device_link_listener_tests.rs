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

fn navigate_to_device_linking(engine: &PlatformAppEngine) {
    engine
        .navigate_to_json_for_test(r#""DeviceLinking""#.into())
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

/// Titles of every surface in the re-composed command batch (main and
/// secondary pane alike), read through the canonical path. The batch is
/// what a shell renders, so title membership is the honest observable
/// for flow state — the retired granular screen_id was Core-internal
/// vocabulary.
fn surface_titles(engine: &PlatformAppEngine) -> Vec<String> {
    let json = engine
        .initial_commands_json()
        .expect("initial_commands_json");
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse commands json");
    v["commands"]
        .as_array()
        .map(|commands| {
            commands
                .iter()
                .filter_map(|c| {
                    c["ReplaceSurface"]["surface"]["title"]
                        .as_str()
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
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
    let _ = engine.navigate_to_json_for_test(r#""Settings""#.into());
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
    let _ = engine.navigate_to_json_for_test(r#""Settings""#.into());
    assert!(
        !engine.device_link_session_is_active_for_test(),
        "session not cancelled after leaving DeviceLinking"
    );
}

// ── Bridge forwarding ──────────────────────────────────────────

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
    assert!(
        surface_titles(&engine).contains(&"Device Wants to Link".to_string()),
        "request_received must present the confirmation surface"
    );
}

// @scenario: pair5_device_link_listener :: on_failed("qr_expired") routes to the expired screen
#[test]
fn qr_expired_bridge_routes_to_qr_expired_screen() {
    let (engine, _dir) = create_engine_with_identity();
    navigate_to_device_linking_quiescent(&engine);
    engine
        .apply_device_link_qr_expired_for_test()
        .expect("apply qr_expired");
    assert!(
        surface_titles(&engine).contains(&"QR Code Expired".to_string()),
        "qr_expired must present the expired surface"
    );
}

// ── Action interception ───────────────────────────────────────

// @scenario: pair5_device_link_listener :: codes_match surfaces DeviceLinkConfirmManual JSON
// M5 B2b: "codes match" is the single confirmation — it emits
// DeviceLinkConfirmManual directly (the redundant proximity step was
// collapsed, 2026-07-03-second-device-join-dead-end item 5).
#[test]
fn codes_match_action_emits_typed_action_result_json() {
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
        .handle_action_json(r#"{"ActionPressed": {"action_id": "codes_match"}}"#.into())
        .expect("codes_match");
    let parsed: serde_json::Value =
        serde_json::from_str(&result_json).expect("parse action result");
    let confirm = parsed
        .get("action_result")
        .and_then(|r| r.get("DeviceLinkConfirmManual"))
        .expect("DeviceLinkConfirmManual variant");
    assert_eq!(confirm.get("code").and_then(|c| c.as_str()), Some("654321"));
    // Engine has advanced to Completing.
    assert!(
        surface_titles(&engine).contains(&"Completing Link".to_string()),
        "codes_match must present the completing surface"
    );
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
    assert!(
        surface_titles(&engine).contains(&"QR Code Expired".to_string()),
        "qr_expired must present the expired surface"
    );

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
    assert!(
        !surface_titles(&engine).contains(&"Device Wants to Link".to_string()),
        "the confirmation surface must be gone after retry rotates the session"
    );
}
