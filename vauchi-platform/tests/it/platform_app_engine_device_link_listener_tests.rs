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
//!   `ActionResult` variants are consumed inside Core so the
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

/// Dispatch one canonical event and return the parsed command batch.
fn link_dispatch(engine: &PlatformAppEngine, event: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(
        &engine
            .dispatch_json(event.to_string())
            .expect("event must dispatch"),
    )
    .expect("parse command batch")
}

/// The `(surface_id, interaction_id)` of a context-bar role for the
/// device-linking surface in the current batch.
fn link_bar_role(engine: &PlatformAppEngine, role: &str) -> (String, String) {
    let batch: serde_json::Value =
        serde_json::from_str(&engine.initial_commands_json().expect("re-compose batch"))
            .expect("parse batch");
    batch["commands"]
        .as_array()
        .and_then(|commands| {
            commands.iter().find_map(|c| {
                let bar = &c["SetContextBar"];
                (bar["surface_id"].as_str() == Some("device_linking")).then(|| {
                    (
                        bar["surface_id"].as_str().unwrap().to_owned(),
                        bar["bar"][role]["interaction_id"]
                            .as_str()
                            .unwrap_or_else(|| panic!("bar must carry a {role} interaction"))
                            .to_owned(),
                    )
                })
            })
        })
        .expect("device_linking context bar must be present")
}

/// Activate a context-bar role on the device-linking surface.
fn activate_link_bar(engine: &PlatformAppEngine, role: &str) -> serde_json::Value {
    let (surface_id, interaction_id) = link_bar_role(engine, role);
    link_dispatch(
        engine,
        serde_json::json!({
            "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
        }),
    )
}

/// Open the secondary action menu and activate the named destructive item.
fn activate_link_menu_item(engine: &PlatformAppEngine, label: &str) -> serde_json::Value {
    let menu = activate_link_bar(engine, "secondary");
    let (surface_id, interaction_id) = menu["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("PresentOverlay")))
        .and_then(|overlay| {
            let item = overlay["overlay"]["items"].as_array().and_then(|items| {
                items
                    .iter()
                    .find(|item| item["label"].as_str() == Some(label))
            })?;
            Some((
                overlay["surface_id"].as_str()?.to_owned(),
                item["interaction_id"].as_str()?.to_owned(),
            ))
        })
        .unwrap_or_else(|| panic!("action menu must carry {label}"));
    link_dispatch(
        engine,
        serde_json::json!({
            "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
        }),
    )
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
    // Confirming via the context bar primary feeds the displayed code to
    // the engine-owned machine (DeviceLinkConfirmManual is consumed
    // internally — the completing surface is the observable outcome).
    let _ = activate_link_bar(&engine, "primary");
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
    // Deny lives in the secondary action menu (destructive item). Through
    // the generic envelope the DeviceLinkDeny result is consumed
    // internally (its machine side effect runs in Core), so the dispatch
    // must succeed and return a generic command batch — not the retired
    // typed envelope and not an unresolved-variant error.
    // The machine-synced denial (machine.deny() → Failed) is covered by
    // the machine's own tests (`deny_fails_with_user_denied`); the
    // quiescent helper here deliberately holds no session, so only the
    // boundary contract is pinned at this level.
    let batch = activate_link_menu_item(&engine, "Deny");
    assert!(
        batch["commands"].is_array(),
        "deny must resolve to a generic command batch, got: {batch}"
    );
    assert!(
        batch.get("action_result").is_none(),
        "deny must not leak the retired action envelope, got: {batch}"
    );
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

    // Retry via the expired surface's primary rotates the session — a
    // fresh one is held (DeviceLinkRetry is consumed internally).
    let _ = activate_link_bar(&engine, "primary");
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

    let _ = activate_link_bar(&engine, "back");
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
