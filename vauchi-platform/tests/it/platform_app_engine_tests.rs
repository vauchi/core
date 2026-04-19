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
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    (engine, dir)
}

/// Drive through the full onboarding flow via JSON actions.
///
/// Sequence mirrors the 6-step onboarding flow:
/// 1. create_new -> default_name
/// 2. TextChanged display_name "Alice" -> updates default_name
/// 3. continue -> groups_setup
/// 4. continue -> contact_info
/// 5. continue -> what_next
/// 6. start_app -> CompleteWith (AppEngine routes to Home)
fn drive_onboarding(engine: &PlatformAppEngine) {
    // Step 1: create_new -> default_name
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("step 1: create_new");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 1");
    assert_eq!(v["NavigateTo"]["screen_id"], "default_name", "step 1");

    // Step 2: enter display name
    let r = engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .expect("step 2: text changed");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 2");
    assert_eq!(v["UpdateScreen"]["screen_id"], "default_name", "step 2");

    // Step 3: continue -> groups_setup
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
        .expect("step 3: continue");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 3");
    assert_eq!(v["NavigateTo"]["screen_id"], "groups_setup", "step 3");

    // Step 4: continue -> contact_info
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
        .expect("step 4: continue");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 4");
    assert_eq!(v["NavigateTo"]["screen_id"], "contact_info", "step 4");

    // Step 5: continue -> what_next
    let r = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
        .expect("step 5: continue");
    let v: serde_json::Value = serde_json::from_str(&r).expect("parse step 5");
    assert_eq!(v["NavigateTo"]["screen_id"], "what_next", "step 5");

    // Step 6: start_app -> CompleteWith (AppEngine transitions to Home)
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start_app"}}"#.into())
        .expect("step 6: start_app");
}

// ============================================================================
// Construction and initial screen
// ============================================================================

// @internal
#[test]
fn new_engine_starts_on_onboarding() {
    let (engine, _dir) = create_engine();
    let json = engine.current_screen_json().expect("screen json");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(screen["screen_id"], "identity_check");
}

// @internal
#[test]
fn current_screen_id_returns_lightweight_id() {
    let (engine, _dir) = create_engine();
    let id = engine.current_screen_id().expect("screen id");
    assert_eq!(id, "identity_check");
}

// ============================================================================
// Onboarding flow
// ============================================================================

// @internal
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

// @internal
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

// @internal
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

// @internal
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
// Notification boundary (CC-05)
// ============================================================================

// @scenario: notification.feature - Poll notifications returns empty before events
// @internal
// @internal
#[test]
fn poll_notifications_returns_empty_before_events() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let notifications = engine.poll_notifications().expect("poll_notifications");
    assert!(
        notifications.is_empty(),
        "poll should return empty when no events dispatched"
    );
}

// @scenario: notification.feature - Drain notifications returns empty before events
// @internal
// @internal
#[test]
fn drain_pending_notifications_returns_empty_before_events() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let notifications = engine
        .drain_pending_notifications()
        .expect("drain_pending_notifications");
    assert!(
        notifications.is_empty(),
        "drain should return empty when no events dispatched"
    );
}

// @scenario: notification.feature - Card update produces no OS notification
// @internal
// @internal
#[test]
fn poll_notifications_after_card_update_returns_no_notification() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    // Add a field to own card — dispatches OwnCardUpdated through the
    // event pipeline. OwnCardUpdated is activity-log-only, so poll
    // should return zero notifications while still processing the event.
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "add_field"}}"#.into())
        .expect("open add field dialog");
    engine
        .handle_action_json(
            r#"{"ListItemSelected": {"component_id": "entry_types", "item_id": "email"}}"#.into(),
        )
        .expect("select email type");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "field_value", "value": "test@example.com"}}"#
                .into(),
        )
        .expect("enter value");
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "submit"}}"#.into())
        .expect("submit field");

    let notifications = engine.poll_notifications().expect("poll_notifications");
    assert!(
        notifications.is_empty(),
        "OwnCardUpdated should not produce a notification, got: {notifications:?}"
    );
}

// @scenario: notification.feature - Poll and drain are independently callable
// @internal
// @internal
#[test]
fn poll_and_drain_are_independently_callable() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    // Both methods should work in sequence without interfering
    let poll_result = engine.poll_notifications().expect("poll");
    let drain_result = engine.drain_pending_notifications().expect("drain");
    let poll_again = engine.poll_notifications().expect("poll again");

    assert!(poll_result.is_empty());
    assert!(drain_result.is_empty());
    assert!(poll_again.is_empty());
}

// ============================================================================
// Error handling
// ============================================================================

// @internal
#[test]
fn handle_action_invalid_json_returns_error() {
    let (engine, _dir) = create_engine();
    let result = engine.handle_action_json("not valid json".into());
    assert!(result.is_err(), "should return error for invalid JSON");
}

// @internal
#[test]
fn navigate_to_invalid_json_returns_error() {
    let (engine, _dir) = create_engine();
    let result = engine.navigate_to_json("not valid json".into());
    assert!(result.is_err(), "should return error for invalid JSON");
}

// ============================================================================
// Invalidation
// ============================================================================

// @internal
#[test]
fn invalidate_all_succeeds() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    engine.invalidate_all().expect("invalidate should succeed");
    let id = engine.current_screen_id().expect("screen id");
    assert_eq!(id, "my_info");
}

// @internal
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

// @internal
#[test]
fn has_identity_returns_false_before_onboarding() {
    let (engine, _dir) = create_engine();
    assert!(!engine.has_identity().expect("has_identity"));
}

// @internal
#[test]
fn has_identity_returns_true_after_onboarding() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    assert!(engine.has_identity().expect("has_identity"));
}

// @internal
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
// @internal
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
// @internal
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

// ============================================================================
// Animated QR frame advancement (ADR-031)
// ============================================================================

/// Extract the `data` string from the first `QrCode` (Display) component in a
/// serialized `ScreenModel` JSON.
fn qr_data_from_screen_json(screen_json: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(screen_json).expect("parse screen");
    for c in v["components"].as_array().expect("components array") {
        if c["QrCode"].is_object() && c["QrCode"]["mode"] == "Display" {
            return c["QrCode"]["data"]
                .as_str()
                .expect("qr data string")
                .to_owned();
        }
    }
    panic!("no QrCode Display in screen: {screen_json}");
}

/// Drive onboarding → Exchange → QR (Hover mode) so the engine is parked on
/// `exchange_show_qr` with an animated-QR session ready to cycle frames.
fn drive_to_show_qr(engine: &PlatformAppEngine) {
    drive_onboarding(engine);
    engine
        .navigate_to_json(r#""Exchange""#.into())
        .expect("navigate to Exchange");
    // Pick Hover mode (standard category) to drop into ShowQr.
    engine
        .handle_action_json(
            r#"{"ListItemSelected": {"component_id": "category:standard", "item_id": "mode:hover"}}"#.into(),
        )
        .expect("select hover mode");
    // Regardless of which ActionResult shape mode-selection returns, the
    // current screen must now be exchange_show_qr.
    let id = engine
        .current_screen_id()
        .expect("screen id after mode select");
    assert_eq!(
        id, "exchange_show_qr",
        "expected show_qr after selecting Hover, got {id}"
    );
}

// @internal
#[test]
fn advance_qr_frame_json_cycles_frames_on_show_qr() {
    let (engine, _dir) = create_engine();
    drive_to_show_qr(&engine);

    let initial = engine.current_screen_json().expect("screen json");
    let initial_data = qr_data_from_screen_json(&initial);

    let next = engine
        .advance_qr_frame_json()
        .expect("advance call")
        .expect("Some(screen) on ShowQr with animated frames");
    let after_data = qr_data_from_screen_json(&next);
    assert_ne!(
        initial_data, after_data,
        "QR frame data must change after advance"
    );
}

// @internal
#[test]
fn advance_qr_frame_json_returns_none_off_exchange_screen() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    // On my_info (not Exchange).
    let result = engine.advance_qr_frame_json().expect("advance call");
    assert!(
        result.is_none(),
        "advance must return None outside the Exchange screen"
    );
}
