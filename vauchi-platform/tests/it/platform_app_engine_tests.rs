// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for PlatformAppEngine.

use std::sync::{Arc, Mutex};

use vauchi_platform::{MobileLocale, PlatformAppEngine, PlatformEventListener};

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

// @internal
#[test]
fn tab_info_pre_identity_returns_only_onboarding() {
    let (engine, _dir) = create_engine();
    let tabs = engine.tab_info(MobileLocale::English).expect("tab info");
    assert_eq!(tabs.len(), 1, "pre-identity should expose just onboarding");
    let tab = &tabs[0];
    assert_eq!(tab.id, "onboarding");
    assert!(!tab.label.is_empty(), "label must be non-empty");
    assert!(!tab.icon.is_empty(), "icon must be non-empty");
    assert_eq!(tab.badge_count, 0);
}

// @internal
#[test]
fn tab_info_post_identity_returns_top_level_tabs() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let tabs = engine.tab_info(MobileLocale::English).expect("tab info");
    let ids: Vec<&str> = tabs.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["my_info", "contacts", "exchange", "groups", "more"],
        "post-identity tab order must be stable for frontends"
    );
    for tab in &tabs {
        assert!(
            !tab.label.is_empty(),
            "label must be non-empty for {}",
            tab.id
        );
        assert!(
            !tab.icon.is_empty(),
            "icon must be non-empty for {}",
            tab.id
        );
    }
}

// @internal
#[test]
fn tab_info_english_labels_come_from_locale() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let tabs = engine.tab_info(MobileLocale::English).expect("tab info");
    let by_id: std::collections::HashMap<_, _> = tabs
        .iter()
        .map(|t| (t.id.as_str(), t.label.as_str()))
        .collect();
    assert_eq!(by_id.get("my_info"), Some(&"My Card"));
    assert_eq!(by_id.get("contacts"), Some(&"Contacts"));
    assert_eq!(by_id.get("exchange"), Some(&"Exchange"));
    assert_eq!(by_id.get("groups"), Some(&"Groups"));
    assert_eq!(by_id.get("more"), Some(&"More"));
}

// @internal
#[test]
fn tab_info_german_labels_differ_from_english_once_locales_loaded() {
    // Non-English locales are only populated after `init_locales()` loads
    // the bundled JSON files — without that, `get_string` falls back to
    // the compile-time-bundled English map. This test exercises the
    // intended production path: init locales, then ask for German.
    let locales_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales");
    vauchi_platform::init_locales(locales_dir.to_string_lossy().to_string()).expect("init_locales");

    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let en = engine.tab_info(MobileLocale::English).expect("en");
    let de = engine.tab_info(MobileLocale::German).expect("de");
    // Same screen IDs in the same order
    let en_ids: Vec<&str> = en.iter().map(|t| t.id.as_str()).collect();
    let de_ids: Vec<&str> = de.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(en_ids, de_ids);
    let en_contacts = en.iter().find(|t| t.id == "contacts").expect("en contacts");
    let de_contacts = de.iter().find(|t| t.id == "contacts").expect("de contacts");
    assert_eq!(en_contacts.label, "Contacts");
    assert_eq!(de_contacts.label, "Kontakte");
}

// @internal
#[test]
fn sidebar_items_pre_identity_returns_only_onboarding() {
    let (engine, _dir) = create_engine();
    let items = engine
        .sidebar_items(MobileLocale::English)
        .expect("sidebar items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "onboarding");
}

// @internal
#[test]
fn sidebar_items_post_identity_is_broader_than_tab_info() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let tabs = engine.tab_info(MobileLocale::English).expect("tab info");
    let items = engine
        .sidebar_items(MobileLocale::English)
        .expect("sidebar items");
    assert!(
        items.len() > tabs.len(),
        "sidebar must be broader than the mobile tab bar (tabs={}, sidebar={})",
        tabs.len(),
        items.len()
    );
    let sidebar_ids: Vec<&str> = items.iter().map(|t| t.id.as_str()).collect();
    // Desktop-specific entries that the mobile tab bar does not expose
    for expected in [
        "settings",
        "recovery",
        "device_management",
        "backup",
        "privacy",
        "support",
        "help",
        "activity_log",
        "sync",
    ] {
        assert!(
            sidebar_ids.contains(&expected),
            "sidebar missing expected entry: {}",
            expected
        );
    }
    for item in &items {
        assert!(!item.label.is_empty(), "label must be non-empty");
        assert!(!item.icon.is_empty(), "icon must be non-empty");
    }
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

// ============================================================================
// Pair 4 — multi-stage exchange engine bridge
// ============================================================================

/// Drive onboarding then navigate to MultiStageExchange so the cached
/// engine is the multi-stage one. Returns the engine ready for bridge
/// pushes.
fn drive_to_multi_stage(engine: &PlatformAppEngine) {
    drive_onboarding(engine);
    engine
        .navigate_to_json(r#""MultiStageExchange""#.into())
        .expect("navigate to MultiStageExchange");
    let id = engine
        .current_screen_id()
        .expect("screen id after navigate");
    assert_eq!(id, "multi_stage_exchange");
}

fn screen_action_ids(json: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(json).expect("parse screen json");
    v["actions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// @internal
#[test]
fn apply_multi_stage_state_finalized_with_session_ended_renders_success_screen() {
    use vauchi_platform::MobileProtocolState;
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    engine
        .apply_multi_stage_state_for_test(MobileProtocolState::Finalized)
        .expect("apply Finalized");
    engine
        .apply_multi_stage_finalized_for_test("Alice".into())
        .expect("apply finalized name");
    engine
        .apply_multi_stage_session_ended_for_test()
        .expect("apply session ended");

    let screen = engine.current_screen_json().expect("screen json");
    assert!(
        screen.contains("Exchange Complete"),
        "session_ended Finalized must render success indicator: {screen}"
    );
    assert!(
        screen.contains("Exchanged with Alice"),
        "success screen must include peer name: {screen}"
    );
    assert_eq!(screen_action_ids(&screen), vec!["done".to_string()]);
}

// @internal
#[test]
fn apply_multi_stage_state_failed_renders_retry_cancel() {
    use vauchi_platform::MobileProtocolState;
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    engine
        .apply_multi_stage_state_for_test(MobileProtocolState::Failed {
            reason: "lost peer".into(),
        })
        .expect("apply Failed");

    let screen = engine.current_screen_json().expect("screen json");
    assert!(
        screen.contains("Exchange Failed"),
        "Failed must render Exchange Failed indicator: {screen}"
    );
    assert!(
        screen.contains("lost peer"),
        "Failed must surface reason detail: {screen}"
    );
    let ids = screen_action_ids(&screen);
    assert!(ids.contains(&"retry".to_string()));
    assert!(ids.contains(&"cancel".to_string()));
}

// @internal
#[test]
fn apply_multi_stage_qr_payload_renders_own_qr_data_in_active_chrome() {
    use vauchi_platform::{MobileProtocolState, MobileQrPayload};
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    engine
        .apply_multi_stage_state_for_test(MobileProtocolState::Advertising)
        .expect("apply Advertising");
    engine
        .apply_multi_stage_qr_payload_for_test(MobileQrPayload {
            data: "vauchi://INIT/zzz".into(),
            error_correction: "L".into(),
            display_duration_ms: 400,
        })
        .expect("apply qr payload");

    let screen = engine.current_screen_json().expect("screen json");
    assert!(
        screen.contains("vauchi://INIT/zzz"),
        "Active chrome must render the bridge-supplied QR data: {screen}"
    );
}

// @internal
#[test]
fn apply_multi_stage_state_is_no_op_when_active_engine_is_not_multi_stage() {
    use vauchi_platform::MobileProtocolState;
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    // On `my_info` (post-onboarding default), not the multi-stage screen.
    let pre_id = engine.current_screen_id().expect("screen id before apply");
    engine
        .apply_multi_stage_state_for_test(MobileProtocolState::Finalized)
        .expect("apply must succeed even when not the active engine");
    let post_id = engine.current_screen_id().expect("screen id after apply");
    assert_eq!(
        pre_id, post_id,
        "bridge push must not affect non-multi-stage screens",
    );
}

// @internal
#[test]
fn picking_glance_from_mode_selection_auto_navigates_to_multi_stage_exchange() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    engine
        .navigate_to_json(r#""Exchange""#.into())
        .expect("navigate to Exchange");
    assert_eq!(
        engine.current_screen_id().expect("screen id"),
        "exchange_mode_selection",
    );
    // User picks Glance — the simplest face-to-face mode. No further
    // frontend call needed: AppEngine routes `StartMultiStageExchange`
    // → `AppScreen::MultiStageExchange`, the platform layer
    // auto-creates the session.
    engine
        .handle_action_json(
            r#"{"ListItemSelected": {"component_id": "category:quick", "item_id": "mode:glance"}}"#
                .into(),
        )
        .expect("select Glance");
    assert_eq!(
        engine.current_screen_id().expect("screen id after select"),
        "multi_stage_exchange",
        "Glance must route through the multi-stage screen — frontend never makes this decision",
    );
}

// @internal
#[test]
fn navigate_to_multi_stage_auto_creates_session_no_frontend_call_needed() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    // Capture invalidation calls — once the session starts the cycle
    // thread fires `on_state_changed(Idle/Advertising)` which the bridge
    // forwards as a `multi_stage_exchange` invalidation.
    let invalidations: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    struct CaptureListener {
        sink: Arc<Mutex<Vec<Vec<String>>>>,
    }
    impl PlatformEventListener for CaptureListener {
        fn on_screens_invalidated(&self, screen_ids: Vec<String>) {
            self.sink.lock().expect("lock").push(screen_ids);
        }
    }
    engine
        .set_event_listener(Box::new(CaptureListener {
            sink: Arc::clone(&invalidations),
        }))
        .expect("set listener");

    engine
        .navigate_to_json(r#""MultiStageExchange""#.into())
        .expect("navigate");
    // Engine is now on multi_stage_exchange. The frontend never asked
    // for a session — core created one.
    assert_eq!(
        engine
            .current_screen_id()
            .expect("screen id after navigate"),
        "multi_stage_exchange",
    );

    // Give the cycle thread a moment to push at least one state.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let calls = invalidations.lock().expect("lock").clone();
    let saw_multi_stage = calls
        .iter()
        .any(|ids| ids.iter().any(|id| id == "multi_stage_exchange"));
    assert!(
        saw_multi_stage,
        "auto-managed session must push at least one invalidation; got {calls:?}",
    );

    // Navigating away cancels the session and stops further pushes.
    engine
        .navigate_back_json()
        .expect("navigate back away from multi_stage_exchange");
    let pre_count = invalidations.lock().expect("lock").len();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let post_count = invalidations.lock().expect("lock").len();
    assert_eq!(
        pre_count, post_count,
        "session must stop pushing after navigate_back; pre={pre_count} post={post_count}",
    );
}

// @internal
#[test]
fn qr_scanned_hardware_event_routes_to_session_when_on_multi_stage_screen() {
    use vauchi_platform::MobileEvent;
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    // QrScanned must not return an ActionResult — the bridge handles
    // the post-scan state push asynchronously. Returning Some here
    // would mean the engine handled it directly (the legacy path),
    // which is the leak we just fixed.
    let result = engine
        .handle_hardware_event(MobileEvent::QrScanned {
            data: "garbage-not-an-init-frame".into(),
        })
        .expect("hardware event accepted");
    assert!(
        result.is_none(),
        "QrScanned on multi_stage_exchange must be routed to session, not the engine — got {result:?}",
    );
}

// @internal
#[test]
fn text_changed_from_peer_scan_routes_to_multi_stage_session() {
    // Pair 4 — the iOS / Android QrCode { mode: Scan } component emits
    // `UserAction::TextChanged { component_id: "peer_scan", value }`
    // on every successful camera decode. On the multi-stage screen the
    // engine itself does not own the cycle-thread session; the platform
    // layer must side-effect-route this scan into
    // `session.process_scanned_qr` the same way it routes the
    // `QrScanned` hardware-event variant. Without that route the scan
    // falls through to the engine's `UpdateScreen` default and is
    // silently dropped.
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    // The action must succeed (not error). The session swallows the
    // garbage payload (no valid init frame) but the route itself runs.
    // Result is `UpdateScreen` — same as the engine's default for an
    // unhandled `TextChanged`. We verify the screen id is unchanged
    // and the action did not error: the side-effect route happens
    // before the engine's fall-through, so a panic / lock-poisoning
    // would surface here.
    let result_json = engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "peer_scan", "value": "garbage-not-an-init-frame"}}"#
                .into(),
        )
        .expect("text changed action accepted");
    let v: serde_json::Value = serde_json::from_str(&result_json).expect("parse action result");
    assert_eq!(
        v["UpdateScreen"]["screen_id"], "multi_stage_exchange",
        "TextChanged from peer_scan must update the multi-stage screen, got {v:?}",
    );
    assert_eq!(
        engine.current_screen_id().expect("current screen"),
        "multi_stage_exchange",
        "scan must not navigate away from multi_stage_exchange",
    );
}

// @internal
#[test]
fn text_changed_from_unknown_component_does_not_panic_on_multi_stage() {
    // Negative case for the auto-route: a `TextChanged` whose
    // `component_id` is not the peer-scan component must NOT call
    // `session.process_scanned_qr` (a different component might emit
    // text — for example a future manual-entry fallback). The action
    // should still succeed and resolve to `UpdateScreen`.
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    let result_json = engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "some_other_field", "value": "hello"}}"#.into(),
        )
        .expect("text changed action accepted");
    let v: serde_json::Value = serde_json::from_str(&result_json).expect("parse action result");
    assert_eq!(
        v["UpdateScreen"]["screen_id"], "multi_stage_exchange",
        "TextChanged from a non-peer-scan component must update the multi-stage screen, got {v:?}",
    );
}

// @internal
#[test]
fn cancel_action_on_multi_stage_screen_stops_session_after_navigate() {
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    // Capture invalidations to detect post-cancel quiesce.
    let invalidations: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    struct CaptureListener {
        sink: Arc<Mutex<Vec<Vec<String>>>>,
    }
    impl PlatformEventListener for CaptureListener {
        fn on_screens_invalidated(&self, screen_ids: Vec<String>) {
            self.sink.lock().expect("lock").push(screen_ids);
        }
    }
    engine
        .set_event_listener(Box::new(CaptureListener {
            sink: Arc::clone(&invalidations),
        }))
        .expect("set listener");

    // Press Cancel — engine returns Complete, AppEngine routes that
    // through handle_completion to navigate_back. After the action
    // resolves the screen has changed away from multi_stage_exchange.
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "cancel"}}"#.into())
        .expect("cancel action");
    let post_screen = engine.current_screen_id().expect("screen id");
    assert_ne!(
        post_screen, "multi_stage_exchange",
        "cancel must navigate away; still on multi_stage_exchange",
    );

    // Wait — no further multi_stage invalidations should arrive.
    let pre_count = invalidations.lock().expect("lock").len();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let post_count = invalidations.lock().expect("lock").len();
    assert_eq!(
        pre_count, post_count,
        "session must be cancelled after handle_action(cancel); pre={pre_count} post={post_count}",
    );
}

// @internal
#[test]
fn apply_multi_stage_state_notifies_event_listener() {
    use vauchi_platform::MobileProtocolState;
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    let invalidations: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));

    struct CaptureListener {
        sink: Arc<Mutex<Vec<Vec<String>>>>,
    }
    impl PlatformEventListener for CaptureListener {
        fn on_screens_invalidated(&self, screen_ids: Vec<String>) {
            self.sink.lock().expect("lock").push(screen_ids);
        }
    }
    engine
        .set_event_listener(Box::new(CaptureListener {
            sink: Arc::clone(&invalidations),
        }))
        .expect("set listener");

    engine
        .apply_multi_stage_state_for_test(MobileProtocolState::Discovered)
        .expect("apply Discovered");

    let calls = invalidations.lock().expect("lock").clone();
    assert!(
        calls
            .iter()
            .any(|ids| ids.iter().any(|id| id == "multi_stage_exchange")),
        "bridge push must notify the listener with multi_stage_exchange screen id; got {calls:?}",
    );
}
