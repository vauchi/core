// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for PlatformAppEngine.

use std::sync::{Arc, Mutex};

use vauchi_platform::{PlatformAppEngine, PlatformEventListener};

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
    assert_eq!(screen["screen_id"], "exchange_mode_selection");
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

// ============================================================================
// Identity and form state queries
// ============================================================================

#[test]
fn has_identity_returns_false_before_onboarding() {
    let (engine, _dir) = create_engine();
    assert!(!engine.has_identity().expect("has_identity"));
}

#[test]
fn has_identity_returns_true_after_onboarding() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    assert!(engine.has_identity().expect("has_identity"));
}

#[test]
fn form_has_data_returns_false_on_non_form_screen() {
    let (engine, _dir) = create_engine();
    assert!(!engine.form_has_data().expect("form_has_data"));
}

// ============================================================================
// Event listener (PlatformEventListener)
// ============================================================================

/// Mock listener that records all `on_screens_invalidated` calls.
struct RecordingListener {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl PlatformEventListener for RecordingListener {
    fn on_screens_invalidated(&self, screen_ids: Vec<String>) {
        self.calls.lock().unwrap().push(screen_ids);
    }
}

// @scenario: event-listener.feature - Event listener receives screen invalidation on card update
#[test]
fn event_listener_receives_invalidation_on_card_update() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    // Register a mock listener
    let calls: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let listener = RecordingListener {
        calls: Arc::clone(&calls),
    };
    engine
        .set_event_listener(Box::new(listener))
        .expect("register listener");

    // Add a field to own card — this triggers OwnCardUpdated event
    // via ContactManager, unlike update_display_name which skips dispatch.
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "add_field"}}"#.into())
        .expect("open add field dialog");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse");
    assert_eq!(
        v["NavigateTo"]["screen_id"], "form_add_field",
        "should open add-field form, got: {v}"
    );

    // Select entry type
    engine
        .handle_action_json(
            r#"{"ListItemSelected": {"component_id": "entry_types", "item_id": "email"}}"#.into(),
        )
        .expect("select email type");

    // Enter field value (required — empty value is rejected)
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "field_value", "value": "test@example.com"}}"#
                .into(),
        )
        .expect("enter value");

    // Submit — calls add_own_field → ContactManager dispatches OwnCardUpdated
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "submit"}}"#.into())
        .expect("submit field");

    // Verify the listener was called with "my_info"
    let recorded = calls.lock().unwrap();
    assert!(
        !recorded.is_empty(),
        "listener should have been called at least once"
    );
    let all_screen_ids: Vec<&String> = recorded.iter().flat_map(|v| v.iter()).collect();
    assert!(
        all_screen_ids.contains(&&"my_info".to_string()),
        "should include my_info screen, got: {all_screen_ids:?}"
    );
}

// @scenario: event-listener.feature - Replacing event listener unregisters previous one
#[test]
fn replacing_event_listener_unregisters_previous() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    // Register first listener
    let calls_1: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let listener_1 = RecordingListener {
        calls: Arc::clone(&calls_1),
    };
    engine
        .set_event_listener(Box::new(listener_1))
        .expect("register first listener");

    // Replace with second listener
    let calls_2: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let listener_2 = RecordingListener {
        calls: Arc::clone(&calls_2),
    };
    engine
        .set_event_listener(Box::new(listener_2))
        .expect("register second listener");

    // Trigger an event via add-field (dispatches OwnCardUpdated)
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "add_field"}}"#.into())
        .expect("open add field dialog");
    engine
        .handle_action_json(
            r#"{"ListItemSelected": {"component_id": "entry_types", "item_id": "phone"}}"#.into(),
        )
        .expect("select phone type");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "field_value", "value": "+1234567890"}}"#.into(),
        )
        .expect("enter value");
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "submit"}}"#.into())
        .expect("submit field");

    // First listener should NOT have been called (unregistered)
    let recorded_1 = calls_1.lock().unwrap();
    assert!(
        recorded_1.is_empty(),
        "first listener should not receive events after replacement, got: {recorded_1:?}"
    );

    // Second listener SHOULD have been called
    let recorded_2 = calls_2.lock().unwrap();
    assert!(
        !recorded_2.is_empty(),
        "second listener should have received events"
    );
}
