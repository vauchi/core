// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for PlatformAppEngine.

use vauchi_platform::PlatformAppEngine;

/// Helper: create a PlatformAppEngine with a temp directory.
fn create_engine() -> (std::sync::Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "wss://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    (engine, dir)
}

/// Drive through the full onboarding flow via JSON actions.
///
/// Sequence mirrors `drive_onboarding` in `vauchi-core/tests/app_engine_tests.rs`:
/// 1. create_new -> welcome
/// 2. get_started -> default_name
/// 3. TextChanged display_name "Alice" -> updates default_name
/// 4. continue -> skip_gate
/// 5. skip_to_finish -> security_explanation
/// 6. continue -> backup_prompt
/// 7. skip -> ready
/// 8. start -> Complete (AppEngine routes to Home)
fn drive_onboarding(engine: &PlatformAppEngine) {
    // Step 1: create_new -> welcome
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("step 1: create_new");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 1");
    assert_eq!(v["NavigateTo"]["screen_id"], "welcome", "step 1");

    // Step 2: get_started -> default_name
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "get_started"}}"#.into())
        .expect("step 2: get_started");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 2");
    assert_eq!(v["NavigateTo"]["screen_id"], "default_name", "step 2");

    // Step 3: enter display name
    let r = engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .expect("step 3: text changed");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 3");
    assert_eq!(v["UpdateScreen"]["screen_id"], "default_name", "step 3");

    // Step 4: continue -> skip_gate
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
        .expect("step 4: continue");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 4");
    assert_eq!(v["NavigateTo"]["screen_id"], "skip_gate", "step 4");

    // Step 5: skip_to_finish -> security_explanation
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "skip_to_finish"}}"#.into())
        .expect("step 5: skip_to_finish");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 5");
    assert_eq!(
        v["NavigateTo"]["screen_id"], "security_explanation",
        "step 5"
    );

    // Step 6: continue -> backup_prompt
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
        .expect("step 6: continue");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 6");
    assert_eq!(v["NavigateTo"]["screen_id"], "backup_prompt", "step 6");

    // Step 7: skip -> ready
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "skip"}}"#.into())
        .expect("step 7: skip");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 7");
    assert_eq!(v["NavigateTo"]["screen_id"], "ready", "step 7");

    // Step 8: start -> Complete (AppEngine transitions to Home)
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start"}}"#.into())
        .expect("step 8: start");
}

// ============================================================================
// Construction and initial screen
// ============================================================================

#[test]
fn new_engine_starts_on_onboarding() {
    let (engine, _dir) = create_engine();
    let json = engine.current_screen_json().expect("screen json");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(screen["screen_id"], "identity_check");
}

#[test]
fn current_screen_id_returns_lightweight_id() {
    let (engine, _dir) = create_engine();
    let id = engine.current_screen_id().expect("screen id");
    assert_eq!(id, "identity_check");
}

// ============================================================================
// Onboarding flow
// ============================================================================

#[test]
fn onboarding_flow_completes_and_transitions_to_my_info() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let id = engine.current_screen_id().expect("screen id");
    assert_eq!(id, "my_info", "should land on my_info after onboarding");
}

// ============================================================================
// Navigation (require completed onboarding)
// ============================================================================

#[test]
fn navigate_to_json_simple_variant() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let result = engine
        .navigate_to_json(r#""Exchange""#.into())
        .expect("navigate");
    let screen: serde_json::Value = serde_json::from_str(&result).expect("parse");
    assert_eq!(screen["screen_id"], "exchange_show_qr");
}

#[test]
fn navigate_back_returns_previous_screen() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    engine
        .navigate_to_json(r#""Exchange""#.into())
        .expect("navigate to exchange");
    let result = engine.navigate_back_json().expect("navigate back");
    let screen: serde_json::Value = serde_json::from_str(&result).expect("parse");
    assert_eq!(screen["screen_id"], "my_info");
}

#[test]
fn available_screens_returns_nav_bar_items() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let json = engine.available_screens_json().expect("available screens");
    let screens: Vec<serde_json::Value> = serde_json::from_str(&json).expect("parse");
    assert!(
        screens.len() >= 4,
        "should have at least 4 nav items, got {screens:?}"
    );
}

// ============================================================================
// Error handling
// ============================================================================

#[test]
fn handle_action_invalid_json_returns_error() {
    let (engine, _dir) = create_engine();
    let result = engine.handle_action_json("not valid json".into());
    assert!(result.is_err(), "should return error for invalid JSON");
}

#[test]
fn navigate_to_invalid_json_returns_error() {
    let (engine, _dir) = create_engine();
    let result = engine.navigate_to_json("not valid json".into());
    assert!(result.is_err(), "should return error for invalid JSON");
}

// ============================================================================
// Invalidation
// ============================================================================

#[test]
fn invalidate_all_succeeds() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    engine.invalidate_all().expect("invalidate should succeed");
    let id = engine.current_screen_id().expect("screen id");
    assert_eq!(id, "my_info");
}

#[test]
fn invalidate_screen_json_succeeds() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    engine
        .invalidate_screen_json(r#""MyInfo""#.into())
        .expect("invalidate screen");
}
