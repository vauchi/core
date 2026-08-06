// allow(large_file)
// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for PlatformAppEngine.

use std::sync::{Arc, Mutex};

use vauchi_platform::{
    MobileBleLinkDirection, MobileEvent, MobileLocale, MobileTabLayout, PlatformAppEngine,
    PlatformAppEngineTestHelpers, PlatformEventListener,
};

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

/// Drive the Add Entry form dialog end-to-end through the canonical
/// envelope: open the secondary action menu, pick the field type from
/// the list surface, fill the value input, and activate the form's
/// primary — the same taps a user makes. Every id is read from the
/// command batches.
fn add_field_via_dialog(engine: &PlatformAppEngine, type_title: &str, value: &str) {
    fn batch_of(result: &str) -> serde_json::Value {
        serde_json::from_str(result).expect("parse command batch")
    }
    fn dispatch(engine: &PlatformAppEngine, event: serde_json::Value) -> serde_json::Value {
        batch_of(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch event"),
        )
    }
    fn first_surface(batch: &serde_json::Value) -> serde_json::Value {
        batch["commands"]
            .as_array()
            .and_then(|commands| {
                commands.iter().find_map(|c| {
                    let surface = &c["ReplaceSurface"]["surface"];
                    surface.is_object().then(|| surface.clone())
                })
            })
            .expect("batch must replace a surface")
    }

    // Report the environment once, as a shell does at boot; responsive
    // surfaces require it before activation.
    let _ = dispatch(
        engine,
        serde_json::json!({"PresentationEnvironmentChanged": {
            "available_width": 900, "available_height": 700,
            "input_modes": ["pointer", "keyboard"], "motion": "full",
        }}),
    );

    let home = batch_of(
        &engine
            .initial_commands_json()
            .expect("re-compose home surface"),
    );
    let home_sid = first_surface(&home)["surface_id"]
        .as_str()
        .expect("home surface id")
        .to_owned();

    // Secondary action menu → Add Entry. The interaction id is
    // revision-scoped and read from the bar, never constructed.
    let secondary_iid = home["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
        .and_then(|bar| bar["bar"]["secondary"]["interaction_id"].as_str())
        .expect("home bar must carry a secondary action menu")
        .to_owned();
    let menu = dispatch(
        engine,
        serde_json::json!({"ActionActivated": {
            "surface_id": home_sid,
            "interaction_id": secondary_iid,
        }}),
    );
    let (overlay_sid, add_entry_id) = menu["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("PresentOverlay")))
        .and_then(|overlay| {
            let item = overlay["overlay"]["items"].as_array().and_then(|items| {
                items
                    .iter()
                    .find(|item| item["label"].as_str() == Some("Add Entry"))
            })?;
            Some((
                overlay["surface_id"].as_str()?.to_owned(),
                item["interaction_id"].as_str()?.to_owned(),
            ))
        })
        .expect("action menu must carry Add Entry");
    let picker = dispatch(
        engine,
        serde_json::json!({"ActionActivated": {
            "surface_id": overlay_sid,
            "interaction_id": add_entry_id,
        }}),
    );

    // The type picker is a new surface: activate it before interacting,
    // then pick the field type row.
    let picker_sid = first_surface(&picker)["surface_id"]
        .as_str()
        .expect("picker surface id")
        .to_owned();
    let _ = dispatch(
        engine,
        serde_json::json!({"SurfaceActivated": { "surface_id": picker_sid }}),
    );
    let type_iid = first_surface(&picker)["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes.iter().find_map(|node| {
                node["List"]["rows"].as_array().and_then(|rows| {
                    rows.iter()
                        .find(|row| row["title"].as_str() == Some(type_title))
                        .and_then(|row| row["activation"]["interaction_id"].as_str())
                        .map(str::to_owned)
                })
            })
        })
        .expect("type picker must list the field type");
    let form = dispatch(
        engine,
        serde_json::json!({"ActionActivated": {
            "surface_id": picker_sid,
            "interaction_id": type_iid,
        }}),
    );

    // Fill the value input, then activate the form's primary (submit).
    let form_surface = first_surface(&form);
    let form_sid = form_surface["surface_id"]
        .as_str()
        .expect("form surface id")
        .to_owned();
    let _ = dispatch(
        engine,
        serde_json::json!({"SurfaceActivated": { "surface_id": form_sid }}),
    );
    let value_binding = form_surface["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes.iter().find_map(|node| {
                node["Input"]["binding_id"]
                    .as_str()
                    .filter(|id| id.contains("field_value"))
                    .map(str::to_owned)
            })
        })
        .expect("form must carry a value input");
    let _ = dispatch(
        engine,
        serde_json::json!({"ValueChanged": {
            "surface_id": form_sid,
            "binding_id": value_binding,
            "value": { "text": value },
        }}),
    );
    let submit_iid = form["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
        .and_then(|bar| bar["bar"]["primary"]["interaction_id"].as_str())
        .expect("form must carry a primary submit")
        .to_owned();
    let _ = dispatch(
        engine,
        serde_json::json!({"ActionActivated": {
            "surface_id": form_sid,
            "interaction_id": submit_iid,
        }}),
    );
}

// ============================================================================
// Construction and initial screen
// ============================================================================

// @internal
fn current_screen_id(engine: &PlatformAppEngine) -> String {
    // initial_commands re-composes the current presentation — the same
    // refresh path a shell hits on load — so the current surface id is
    // readable without the retired screen seam.
    let json = engine
        .initial_commands_json()
        .expect("initial_commands_json");
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse commands json");
    v["commands"]
        .as_array()
        .and_then(|commands| {
            commands.iter().find_map(|c| {
                c["ReplaceSurface"]["surface"]["surface_id"]
                    .as_str()
                    .map(str::to_owned)
            })
        })
        .unwrap_or_default()
}

/// Title of the current top surface, read through the canonical
/// re-composition path. The title is Core-prepared copy a shell renders
/// verbatim, so it is the honest observable for "which screen state" —
/// the retired granular screen_id was Core-internal vocabulary.
fn current_surface_title(engine: &PlatformAppEngine) -> String {
    let json = engine
        .initial_commands_json()
        .expect("initial_commands_json");
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse commands json");
    v["commands"]
        .as_array()
        .and_then(|commands| {
            commands.iter().find_map(|c| {
                c["ReplaceSurface"]["surface"]["title"]
                    .as_str()
                    .map(str::to_owned)
            })
        })
        .unwrap_or_default()
}

#[test]
fn new_engine_starts_on_onboarding() {
    let (engine, _dir) = create_engine();
    assert_eq!(current_screen_id(&engine), "onboarding");
}

// @internal
#[test]
fn initial_screen_id_is_identity_check() {
    let (engine, _dir) = create_engine();
    let id = current_screen_id(&engine);
    // The presentation surface id for the whole onboarding flow —
    // "identity_check" was the retired Core-internal screen id.
    assert_eq!(id, "onboarding");
}

// @internal
#[test]
fn nav_items_mobile_pre_identity_returns_only_onboarding() {
    let (engine, _dir) = create_engine();
    let tabs = engine
        .nav_items(MobileTabLayout::Mobile, MobileLocale::English)
        .expect("nav items");
    assert_eq!(tabs.len(), 1, "pre-identity should expose just onboarding");
    let tab = &tabs[0];
    assert_eq!(tab.id, "onboarding");
    assert!(!tab.label.is_empty(), "label must be non-empty");
    assert!(!tab.icon.is_empty(), "icon must be non-empty");
    assert_eq!(tab.badge_count, 0);
}

// @internal
#[test]
fn nav_items_mobile_post_identity_returns_top_level_tabs() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let tabs = engine
        .nav_items(MobileTabLayout::Mobile, MobileLocale::English)
        .expect("nav items");
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
fn nav_items_mobile_english_labels_come_from_locale() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let tabs = engine
        .nav_items(MobileTabLayout::Mobile, MobileLocale::English)
        .expect("nav items");
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
fn nav_items_mobile_german_labels_differ_from_english_once_locales_loaded() {
    // Non-English locales are only populated after `init_locales()` loads
    // the bundled JSON files — without that, `get_string` falls back to
    // the compile-time-bundled English map. This test exercises the
    // intended production path: init locales, then ask for German.
    let locales_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales");
    vauchi_platform::init_locales(locales_dir.to_string_lossy().to_string()).expect("init_locales");

    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let en = engine
        .nav_items(MobileTabLayout::Mobile, MobileLocale::English)
        .expect("en");
    let de = engine
        .nav_items(MobileTabLayout::Mobile, MobileLocale::German)
        .expect("de");
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
fn nav_items_dispatches_on_layout() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let mobile = engine
        .nav_items(MobileTabLayout::Mobile, MobileLocale::English)
        .expect("mobile nav");
    let desktop = engine
        .nav_items(MobileTabLayout::Desktop, MobileLocale::English)
        .expect("desktop nav");
    let mobile_ids: Vec<String> = mobile.iter().map(|t| t.id.clone()).collect();
    let desktop_ids: Vec<String> = desktop.iter().map(|t| t.id.clone()).collect();
    assert_eq!(
        mobile_ids,
        vec!["my_info", "contacts", "exchange", "groups", "more"],
        "nav_items(Mobile) must return the five-tab mobile bar"
    );
    assert!(
        desktop_ids.contains(&"settings".to_string()),
        "nav_items(Desktop) must expose desktop-only entries the mobile bar omits"
    );
    assert!(
        desktop.len() > mobile.len(),
        "desktop sidebar must be broader than the mobile bar (mobile={}, desktop={})",
        mobile.len(),
        desktop.len()
    );
}

// @internal
#[test]
fn nav_items_desktop_pre_identity_returns_only_onboarding() {
    let (engine, _dir) = create_engine();
    let items = engine
        .nav_items(MobileTabLayout::Desktop, MobileLocale::English)
        .expect("desktop nav");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "onboarding");
}

// @internal
#[test]
fn nav_items_desktop_post_identity_is_broader_than_mobile() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let tabs = engine
        .nav_items(MobileTabLayout::Mobile, MobileLocale::English)
        .expect("mobile nav");
    let items = engine
        .nav_items(MobileTabLayout::Desktop, MobileLocale::English)
        .expect("desktop nav");
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
    let id = current_screen_id(&engine);
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
        .navigate_to_json_for_test(r#""Exchange""#.into())
        .expect("navigate");
    let envelope: serde_json::Value = serde_json::from_str(&result).expect("parse");
    // The wire envelope mobile frontends consume: the Exchange tab root
    // reports the canonical `exchange` id so the bottom nav bar renders.
    assert_eq!(envelope["screen"]["screen_id"], "exchange");
}

// @scenario: exchange.feature :: Multi-stage exchange entry surfaces lifecycle commands in envelope
#[test]
fn navigate_to_json_envelope_carries_lifecycle_commands() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    // First nav lands on MultiStageExchange — its `screen_entered`
    // hook emits three commands (brightness, idle timer, orientation
    // lock). The previous engine's `screen_exited` is the default
    // empty.
    let result = engine
        .navigate_to_json_for_test(r#"{"MultiStageExchange":{"mode":"glance"}}"#.into())
        .expect("navigate to multi-stage");
    let envelope: serde_json::Value = serde_json::from_str(&result).expect("parse");
    assert_eq!(envelope["screen"]["screen_id"], "multi_stage_exchange");
    let commands = envelope["commands"]
        .as_array()
        .expect("envelope.commands array");
    assert_eq!(
        commands.len(),
        4,
        "expected 4 lifecycle commands; got {commands:?}"
    );
    assert_eq!(
        commands[0]["SetScreenBrightness"]["level"], 0.65,
        "first command must dim brightness; got {commands:?}",
    );
    assert_eq!(
        commands[1]["SetIdleTimerDisabled"]["disabled"], true,
        "second command must disable idle timer; got {commands:?}",
    );
    assert_eq!(
        commands[2]["SetOrientationLock"]["orientation"], "Portrait",
        "third command must lock portrait; got {commands:?}",
    );
    // Phase 1.B of `2026-05-11-hover-graduation-plan.md` — engine announces
    // the camera selector explicitly. Default routing today is
    // `new_glance()` (back camera); Phase 1.E adds the Hover→front
    // mode-dispatch.
    assert_eq!(
        commands[3]["SwitchCamera"]["use_front"], false,
        "fourth command must announce camera selector; got {commands:?}",
    );
}

// @internal
#[test]
fn poll_notifications_after_card_update_returns_no_notification() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    // Add a field to own card — dispatches OwnCardUpdated through the
    // event pipeline. OwnCardUpdated is activity-log-only, so poll
    // should return zero notifications while still processing the event.
    add_field_via_dialog(&engine, "Email", "test@example.com");

    let notifications = engine.poll_notifications().expect("poll_notifications");
    assert!(
        notifications.is_empty(),
        "OwnCardUpdated should not produce a notification, got: {notifications:?}"
    );
}

// @internal
#[test]
fn on_wakeup_returns_envelope_with_schedule_wakeup_command() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    let envelope_json = engine.on_wakeup().expect("on_wakeup");
    let envelope: serde_json::Value =
        serde_json::from_str(&envelope_json).expect("parse on_wakeup envelope");

    assert!(
        envelope.get("notifications").is_some(),
        "envelope must carry notifications array"
    );
    assert!(
        envelope.get("commands").is_some(),
        "envelope must carry commands array"
    );

    let scheduled: Vec<&serde_json::Value> = envelope["commands"]
        .as_array()
        .expect("commands array")
        .iter()
        .filter(|c| c.get("ScheduleWakeup").is_some())
        .collect();
    assert_eq!(
        scheduled.len(),
        1,
        "on_wakeup must emit exactly one ScheduleWakeup command, got {commands:?}",
        commands = envelope["commands"]
    );
}

// ============================================================================
// Error handling
// ============================================================================

// @internal
#[test]
fn dispatch_invalid_json_returns_error() {
    let (engine, _dir) = create_engine();
    let result = engine.dispatch_json("not valid json".into());
    assert!(result.is_err(), "should return error for invalid JSON");
}

// @internal
#[test]
fn navigate_to_invalid_json_returns_error() {
    let (engine, _dir) = create_engine();
    let result = engine.navigate_to_json_for_test("not valid json".into());
    assert!(result.is_err(), "should return error for invalid JSON");
}

// ============================================================================
// Canonical presentation invalidation
// ============================================================================

// @internal
#[test]
fn presentation_invalidation_succeeds() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    engine
        .dispatch_json(r#""PresentationInvalidated""#.into())
        .expect("presentation invalidation should succeed");
    let id = current_screen_id(&engine);
    assert_eq!(id, "my_info");
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

// ============================================================================
// Event listener (PlatformEventListener)
// ============================================================================

/// Mock listener that records presentation invalidation calls.
struct RecordingListener {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl PlatformEventListener for RecordingListener {
    fn on_presentation_invalidated(&self) {
        self.calls.lock().unwrap().push(Vec::new());
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
    add_field_via_dialog(&engine, "Email", "test@example.com");

    // Verify the generic presentation listener was called.
    let recorded = calls.lock().unwrap();
    assert!(
        !recorded.is_empty(),
        "listener should have been called at least once"
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
    add_field_via_dialog(&engine, "Phone", "+1234567890");

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

/// Extract the payload of the first display-purpose `Qr` node in the
/// re-composed command batch — the QR a shell would render on-screen.
fn qr_data_from_batch(engine: &PlatformAppEngine) -> String {
    fn find_qr_payload(nodes: &[serde_json::Value]) -> Option<String> {
        nodes.iter().find_map(|node| {
            let qr = &node["Qr"];
            if qr.is_object() && qr["purpose"].as_str() == Some("display") {
                return qr["payloads"][0].as_str().map(str::to_owned);
            }
            node["Group"]["children"]
                .as_array()
                .and_then(|children| find_qr_payload(children))
        })
    }
    let json = engine.initial_commands_json().expect("re-compose batch");
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse batch");
    v["commands"]
        .as_array()
        .and_then(|commands| {
            commands.iter().find_map(|c| {
                c["ReplaceSurface"]["surface"]["nodes"]
                    .as_array()
                    .and_then(|nodes| find_qr_payload(nodes))
            })
        })
        .unwrap_or_else(|| panic!("no display Qr node in batch: {json}"))
}

/// Report a successful camera decode the way a shell does: a ValueChanged
/// on the capture-purpose Qr node's binding in the current batch. Sets
/// camera + BLE capabilities first so the capture node renders.
fn scan_qr_via_capture_binding(engine: &PlatformAppEngine, data: &str) {
    fn find_capture_binding(nodes: &[serde_json::Value]) -> Option<String> {
        nodes.iter().find_map(|node| {
            let qr = &node["Qr"];
            if qr.is_object() && qr["purpose"].as_str() == Some("capture") {
                return qr["id"].as_str().map(str::to_owned);
            }
            node["Group"]["children"]
                .as_array()
                .and_then(|children| find_capture_binding(children))
        })
    }

    let json = engine.initial_commands_json().expect("re-compose batch");
    let batch: serde_json::Value = serde_json::from_str(&json).expect("parse batch");
    let (surface_id, binding_id) = batch["commands"]
        .as_array()
        .and_then(|commands| {
            commands.iter().find_map(|c| {
                let surface = &c["ReplaceSurface"]["surface"];
                let binding = surface["nodes"]
                    .as_array()
                    .and_then(|nodes| find_capture_binding(nodes))?;
                Some((surface["surface_id"].as_str()?.to_owned(), binding))
            })
        })
        .unwrap_or_else(|| panic!("no capture Qr node in batch: {json}"));

    let event = serde_json::json!({
        "ValueChanged": {
            "surface_id": surface_id,
            "binding_id": binding_id,
            "value": { "text": data },
        }
    });
    let _ = engine
        .dispatch_json(event.to_string())
        .expect("scan ValueChanged must dispatch");
}

/// Select an exchange mode through the canonical envelope: expand the
/// "Other ways to connect" row on the exchange surface, then activate the
/// mode's list row — ids read from the batches, never constructed.
fn select_exchange_mode(engine: &PlatformAppEngine, mode_title: &str) {
    fn dispatch(engine: &PlatformAppEngine, event: serde_json::Value) -> serde_json::Value {
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch event"),
        )
        .expect("parse command batch")
    }
    fn find_row(nodes: &[serde_json::Value], title: &str) -> Option<String> {
        nodes.iter().find_map(|node| {
            node["List"]["rows"].as_array().and_then(|rows| {
                rows.iter()
                    .find(|row| row["title"].as_str() == Some(title))
                    .and_then(|row| row["activation"]["interaction_id"].as_str())
                    .map(str::to_owned)
            })
        })
    }

    let _ = dispatch(
        engine,
        serde_json::json!({"PresentationEnvironmentChanged": {
            "available_width": 900, "available_height": 700,
            "input_modes": ["pointer", "keyboard"], "motion": "full",
        }}),
    );
    // Re-compose first: the responsive coordinator only learns the visible
    // panes from a composed batch, and SurfaceActivated rejects a surface
    // it has not seen yet.
    let batch: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("re-compose exchange surface"),
    )
    .expect("parse exchange batch");
    let _ = dispatch(
        engine,
        serde_json::json!({"SurfaceActivated": { "surface_id": "exchange" }}),
    );
    let expand_iid = batch["commands"]
        .as_array()
        .and_then(|commands| {
            commands.iter().find_map(|c| {
                c["ReplaceSurface"]["surface"]["nodes"]
                    .as_array()
                    .and_then(|nodes| find_row(nodes, "Other ways to connect"))
            })
        })
        .expect("exchange surface must list Other ways to connect");
    let expanded = dispatch(
        engine,
        serde_json::json!({"ActionActivated": {
            "surface_id": "exchange",
            "interaction_id": expand_iid,
        }}),
    );
    let mode_iid = expanded["commands"]
        .as_array()
        .and_then(|commands| {
            commands.iter().find_map(|c| {
                c["ReplaceSurface"]["surface"]["nodes"]
                    .as_array()
                    .and_then(|nodes| find_row(nodes, mode_title))
            })
        })
        .unwrap_or_else(|| panic!("expanded exchange surface must list {mode_title}"));
    let _ = dispatch(
        engine,
        serde_json::json!({"ActionActivated": {
            "surface_id": "exchange",
            "interaction_id": mode_iid,
        }}),
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
        .navigate_to_json_for_test(r#"{"MultiStageExchange":{"mode":"glance"}}"#.into())
        .expect("navigate to MultiStageExchange");
    let id = current_screen_id(engine);
    assert_eq!(id, "multi_stage_exchange");
}

// @internal
#[test]
fn picking_glance_from_mode_selection_auto_navigates_to_ble_exchange() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    engine
        .navigate_to_json_for_test(r#""Exchange""#.into())
        .expect("navigate to Exchange");
    assert_eq!(current_screen_id(&engine), "exchange",);
    // User picks Glance — G3: the quick mode is one-sided QR + BLE. No
    // further frontend call needed: AppEngine routes `StartBleExchange`
    // → `AppScreen::BleExchange`, the platform layer drives the BLE flow.
    select_exchange_mode(&engine, "Glance");
    // G3: the Glance surface carries its title and the one-sided OOB QR
    // node (generated once on entry) so the exposure-closing pin material
    // is on-screen, not aspirational. "exchange_ble_glance" was the retired
    // Core-internal screen id; the Qr node is what a shell actually renders.
    assert_eq!(
        current_surface_title(&engine),
        "Glance",
        "Glance must route through its one-sided-QR BLE screen (G3) — frontend never makes this decision",
    );
    let batch_json = engine.initial_commands_json().expect("glance batch");
    let batch: serde_json::Value = serde_json::from_str(&batch_json).expect("parse glance batch");
    let has_qr = batch["commands"]
        .as_array()
        .map(|commands| commands.iter().any(|c| c.to_string().contains(r#""Qr""#)))
        .unwrap_or(false);
    assert!(
        has_qr,
        "the Glance screen must render the one-sided QR, got: {batch_json}"
    );
}

// @internal
#[test]
fn navigate_to_multi_stage_auto_creates_session_no_frontend_call_needed() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    // Capture invalidation calls. Pre-32m the cycle thread fired
    // these from a background bridge listener; post-T1.2c the
    // platform's `poll_notifications` wrapper fires one synchronously
    // whenever the machine advanced while on the multi-stage screen.
    // The test now polls explicitly to trigger the advance.
    let invalidations: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    struct CaptureListener {
        sink: Arc<Mutex<Vec<Vec<String>>>>,
    }
    impl PlatformEventListener for CaptureListener {
        fn on_presentation_invalidated(&self) {
            self.sink.lock().expect("lock").push(Vec::new());
        }
    }
    engine
        .set_event_listener(Box::new(CaptureListener {
            sink: Arc::clone(&invalidations),
        }))
        .expect("set listener");

    engine
        .navigate_to_json_for_test(r#"{"MultiStageExchange":{"mode":"glance"}}"#.into())
        .expect("navigate");
    // Engine is now on multi_stage_exchange. The frontend never asked
    // for a session — core created one.
    assert_eq!(current_screen_id(&engine), "multi_stage_exchange",);

    // Poll once — this advances the AppEngine-owned multi-stage
    // machine and the platform wrapper fires the invalidation
    // synchronously (T1.2c contract). A single poll is sufficient;
    // wall-clock sleeps are not needed (CC-06 — no real waits in
    // tests, the cycle thread that needed them is retired).
    engine.poll_notifications().expect("poll");
    let calls = invalidations.lock().expect("lock").clone();
    assert!(
        !calls.is_empty(),
        "auto-managed session must push at least one invalidation on poll; got {calls:?}",
    );

    // Navigating away cancels the session — subsequent polls fire no
    // multi-stage invalidations because `multi_stage_session_active()`
    // returns false.
    engine
        .navigate_back_json()
        .expect("navigate back away from multi_stage_exchange");
    let pre_count = invalidations.lock().expect("lock").len();
    engine.poll_notifications().expect("poll");
    engine.poll_notifications().expect("poll");
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

    // Hardware events use the same Event -> Command boundary as every other
    // shell interaction. The multi-stage bridge may handle the scan
    // internally, but it must still return a generic presentation batch and
    // never expose the retired ActionResult envelope.
    let result_json = engine
        .handle_hardware_event(MobileEvent::QrScanned {
            data: "garbage-not-an-init-frame".into(),
        })
        .expect("hardware event accepted");
    let v: serde_json::Value =
        serde_json::from_str(&result_json).expect("parse hardware event envelope");
    assert!(
        v.get("action_result").is_none(),
        "legacy result escaped: {v:?}"
    );
    assert!(
        v["commands"].is_array(),
        "hardware event must return a generic command envelope, got {v:?}",
    );
}

// @internal
#[test]
fn glance_scanning_a_peer_qr_connects_only_to_that_peer() {
    // The full exposure-closer live path (2026-06-10-ble-unauthenticated-peer-
    // identity): device A scans device B's one-sided Glance QR, then its BLE
    // scan discovers B advertising — A connects specifically to B. A foreign
    // advertiser A never scanned is ignored (no tiebreak, no latch race → F1).
    use vauchi_core::exchange::oob_bootstrap::OobBootstrapQr;
    use vauchi_platform::MobileEvent;

    let (a, _da) = create_engine();
    drive_onboarding(&a);
    let (b, _db) = create_engine();
    drive_onboarding(&b);

    for e in [&a, &b] {
        e.set_device_capabilities_json(r#"{"has_camera": true, "has_ble": true}"#.into())
            .expect("report camera + BLE capabilities at boot");
        e.navigate_to_json_for_test(r#""Exchange""#.into())
            .expect("navigate to Exchange");
        select_exchange_mode(e, "Glance");
    }

    // B displays its QR; parse it to learn B's advertised identity.
    let b_qr = qr_data_from_batch(&b);
    let b_identity = *OobBootstrapQr::from_data_string(&b_qr)
        .expect("B's QR parses")
        .identity_key();

    // A scans B's QR (the camera Component emits TextChanged on the scan id).
    scan_qr_via_capture_binding(&a, &b_qr);

    // A's BLE scan first surfaces a FOREIGN advertiser A never scanned → ignored.
    let foreign = a
        .handle_hardware_event(MobileEvent::BleDeviceDiscovered {
            id: "mallory-device".into(),
            rssi: -40,
            adv_data: vec![0xAB; 32],
        })
        .expect("foreign discovery accepted");
    assert!(
        !foreign.contains("mallory-device"),
        "A must ignore an advertiser it did not scan (F1 dissolves), got: {foreign}",
    );

    // Then B is discovered advertising the scanned identity → A connects to B.
    let discovery = a
        .handle_hardware_event(MobileEvent::BleDeviceDiscovered {
            id: "b-device".into(),
            rssi: -40,
            adv_data: b_identity.to_vec(),
        })
        .expect("B discovery accepted");
    assert!(
        discovery.contains("BleConnect") && discovery.contains("b-device"),
        "A must connect specifically to the scanned peer, got: {discovery}",
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
    // A scan from the on-screen capture component must dispatch without
    // error and route into the session without navigating away.
    scan_qr_via_capture_binding(&engine, "garbage-not-an-init-frame");
    assert_eq!(
        current_screen_id(&engine),
        "multi_stage_exchange",
        "scan must not navigate away from multi_stage_exchange",
    );
}

// @internal
#[test]
fn biometric_unlock_succeeded_hardware_event_returns_authentication_command_when_no_duress() {
    // ADR-031 routing test for the biometric unlock pathway. A fresh
    // engine has no identity and no duress PIN configured, so
    // `is_duress_enabled()` returns false and the outcome must be
    // `Unlocked`. The `PromptForDuressPin` branch is exercised at
    // the `Vauchi::biometric_unlock_decision` layer in
    // `core/vauchi-core/src/api/vauchi/security.rs` tests; here we
    // assert only that the hardware event is routed to that decision
    // path and that its decision returns as a generic authentication
    // command. Retires the legacy
    // `PlatformAppEngine::biometric_unlock_check` typed getter
    // (Track B of `2026-05-11-pure-functional-core-program`).
    use vauchi_platform::MobileEvent;
    let (engine, _dir) = create_engine();

    let result_json = engine
        .handle_hardware_event(MobileEvent::BiometricUnlockSucceeded)
        .expect("biometric event accepted");
    let v: serde_json::Value =
        serde_json::from_str(&result_json).expect("parse biometric result envelope");
    assert!(
        v.get("action_result").is_none(),
        "legacy result escaped: {v:?}"
    );
    assert_eq!(
        v["commands"][0]["SetAuthenticationRequirement"]["requirement"], "unlocked",
        "fresh engine without duress must yield an unlocked command, got {v:?}",
    );
}

// Negative case for the auto-route: a ValueChanged that names no
// binding on the visible surface must fail closed at the parse
// boundary — it neither routes to the session nor silently updates.
// (The retired TextChanged path resolved unknown components to a
// default UpdateScreen; the canonical boundary is deliberately
// stricter.)
// @internal
#[test]
fn value_changed_with_unknown_binding_fails_closed_on_multi_stage() {
    let (engine, _dir) = create_engine();
    drive_to_multi_stage(&engine);

    let result = engine.dispatch_json(
        serde_json::json!({
            "ValueChanged": {
                "surface_id": "multi_stage_exchange",
                "binding_id": "no.such.binding",
                "value": { "text": "hello" },
            }
        })
        .to_string(),
    );
    assert!(
        result.is_err(),
        "unknown binding must fail closed, got: {result:?}"
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
        fn on_presentation_invalidated(&self) {
            self.sink.lock().expect("lock").push(Vec::new());
        }
    }
    engine
        .set_event_listener(Box::new(CaptureListener {
            sink: Arc::clone(&invalidations),
        }))
        .expect("set listener");

    // Tap the context bar's back interaction — Core routes completion
    // through navigate_back. After the activation the surface has
    // changed away from multi_stage_exchange.
    let batch: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("re-compose multi-stage surface"),
    )
    .expect("parse batch");
    let (bar_surface, back_id) = batch["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
        .and_then(|bar| {
            Some((
                bar["surface_id"].as_str()?.to_owned(),
                bar["bar"]["back"]["interaction_id"].as_str()?.to_owned(),
            ))
        })
        .expect("multi-stage bar must carry a back interaction");
    engine
        .dispatch_json(
            serde_json::json!({
                "ActionActivated": { "surface_id": bar_surface, "interaction_id": back_id }
            })
            .to_string(),
        )
        .expect("back activation");
    let post_screen = current_screen_id(&engine);
    assert_ne!(
        post_screen, "multi_stage_exchange",
        "cancel must navigate away; still on multi_stage_exchange",
    );

    // Explicit polls advance any active session synchronously. Once cancel has
    // removed the session they must not produce another invalidation.
    let pre_count = invalidations.lock().expect("lock").len();
    engine.poll_notifications().expect("first post-cancel poll");
    engine
        .poll_notifications()
        .expect("second post-cancel poll");
    let post_count = invalidations.lock().expect("lock").len();
    assert_eq!(
        pre_count, post_count,
        "session must be cancelled after handle_action(cancel); pre={pre_count} post={post_count}",
    );
}

// @scenario: exchange-ble.feature - Terminal BLE machine events surface to the frontend
// @internal
#[test]
fn ble_machine_terminal_event_fires_invalidation_and_flips_chrome() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    let calls: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    engine
        .set_event_listener(Box::new(RecordingListener {
            calls: Arc::clone(&calls),
        }))
        .expect("register listener");

    engine
        .navigate_to_json_for_test(r#"{"BleExchange":{"mode":"magic"}}"#.into())
        .expect("navigate to BLE exchange");

    // Peer token 0xff… sorts above our identity-derived token, so this
    // device is the initiator and the discovery builds the
    // AppEngine-owned handshake session.
    engine
        .handle_hardware_event(MobileEvent::BleDeviceDiscovered {
            id: "AA:BB:CC:DD:EE:FF".into(),
            rssi: -40,
            adv_data: vec![0xff, 0xff, 0xff, 0xff],
        })
        .expect("discovery");

    // A malformed data chunk drives the machine to a terminal Failed
    // event. Without the chrome flip + invalidation, a terminal machine
    // event was invisible: the frontend kept rendering "Exchanging..."
    // forever (P5b re-test, `2026-06-06-android-ble-execution`).
    engine
        .handle_hardware_event(MobileEvent::BleCharacteristicNotified {
            device_id: "peer-1".into(),
            direction: MobileBleLinkDirection::Outbound,
            uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567897".into(),
            data: vec![0u8; 8],
        })
        .expect("malformed chunk");

    assert_eq!(
        current_surface_title(&engine),
        "Failed",
        "terminal machine failure must flip the chrome to the failed screen"
    );
    assert!(
        !calls.lock().unwrap().is_empty(),
        "terminal BLE machine event must fire a presentation invalidation"
    );
}

// @internal
#[test]
fn poll_notifications_on_ble_discovery_fires_invalidation() {
    // Second half of the wait-forever fix (`2026-06-11-exchange-waits-forever`):
    // a bounded-wait BLE discovery times out in core via the `tick` inside
    // `poll_notifications` — but unlike a hardware-event terminal (covered by
    // `ble_machine_terminal_event_fires_invalidation_and_flips_chrome`), the
    // pre-fix `poll_notifications` fired NO invalidation for the BLE screen, so
    // a flipped `Failed` screen never reached the frontend and "Searching…"
    // waited forever. Assert a bare poll on the BLE screen now invalidates, so
    // the listener's unconditional `loadScreen()` surfaces a tick-driven flip.
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    let calls: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    engine
        .set_event_listener(Box::new(RecordingListener {
            calls: Arc::clone(&calls),
        }))
        .expect("register listener");

    engine
        .navigate_to_json_for_test(r#"{"BleExchange":{"mode":"magic"}}"#.into())
        .expect("navigate to BLE exchange");
    calls.lock().unwrap().clear(); // ignore navigation-time fires

    // No hardware event — just a bare poll, the cadence the frontend pump runs.
    engine.poll_notifications().expect("poll");

    assert!(
        !calls.lock().unwrap().is_empty(),
        "a bare poll on the BLE discovering screen must fire \
         a presentation invalidation so a tick-driven timeout surfaces; got {:?}",
        calls.lock().unwrap()
    );
}

// @internal
#[test]
fn ble_discovery_via_dispatch_json_builds_the_same_session_as_the_typed_seam() {
    // ADR-066 admits one Event input. Today the BLE handshake session is
    // built only inside PlatformAppEngine::handle_hardware_event — an event
    // arriving through the canonical dispatch_json envelope misses the
    // session-building routing. Two identically-driven engines must produce
    // the same observable outcome (a BleConnect command for the tiebreak
    // winner) whichever envelope carries the same discovery.
    let (typed, _dt) = create_engine();
    drive_onboarding(&typed);
    typed
        .navigate_to_json_for_test(r#"{"BleExchange":{"mode":"magic"}}"#.into())
        .expect("typed: navigate to BLE exchange");

    let (canonical, _dc) = create_engine();
    drive_onboarding(&canonical);
    canonical
        .navigate_to_json_for_test(r#"{"BleExchange":{"mode":"magic"}}"#.into())
        .expect("canonical: navigate to BLE exchange");

    // Peer token 0xff… sorts above either engine's identity-derived token,
    // so each engine is the initiator and the discovery must build the
    // AppEngine-owned handshake session.
    let typed_json = typed
        .handle_hardware_event(MobileEvent::BleDeviceDiscovered {
            id: "AA:BB:CC:DD:EE:FF".into(),
            rssi: -40,
            adv_data: vec![0xff, 0xff, 0xff, 0xff],
        })
        .expect("typed seam accepts discovery");
    assert!(
        typed_json.contains("BleConnect"),
        "typed seam must emit BleConnect for the tiebreak winner, got: {typed_json}",
    );

    let canonical_json = canonical
        .dispatch_json(
            r#"{"BleDeviceDiscovered": {"id": "AA:BB:CC:DD:EE:FF", "rssi": -40, "adv_data": [255, 255, 255, 255]}}"#
                .into(),
        )
        .expect("canonical envelope accepts discovery");
    assert!(
        canonical_json.contains("BleConnect"),
        "dispatch_json must route the discovery exactly like the typed seam \
         (ADR-066: one Event input), got: {canonical_json}",
    );
}

// @internal
#[test]
fn qr_scanned_via_dispatch_json_routes_to_multi_stage_session_like_the_typed_seam() {
    // Pair 4 — on the multi-stage screen the typed seam auto-routes a scan
    // into the live session (the frontend never knows a session exists).
    // The canonical envelope must route identically: an event that only
    // reaches the generic reducer would surface as an unknown-session
    // failure instead of a presentation batch.
    let (typed, _dt) = create_engine();
    drive_to_multi_stage(&typed);
    let (canonical, _dc) = create_engine();
    drive_to_multi_stage(&canonical);

    let typed_json = typed
        .handle_hardware_event(MobileEvent::QrScanned {
            data: "garbage-not-an-init-frame".into(),
        })
        .expect("typed seam accepts scan");

    let canonical_json = canonical
        .dispatch_json(r#"{"QrScanned": {"data": "garbage-not-an-init-frame"}}"#.into())
        .expect("canonical envelope accepts scan");

    for (label, json) in [("typed", &typed_json), ("canonical", &canonical_json)] {
        let v: serde_json::Value = serde_json::from_str(json).expect("parse envelope");
        assert!(
            v.get("action_result").is_none(),
            "{label}: legacy result escaped: {v:?}"
        );
        assert!(
            v["commands"].is_array(),
            "{label}: scan must return a generic command envelope, got {v:?}",
        );
    }
    assert_eq!(
        typed_json, canonical_json,
        "the canonical envelope must route the scan exactly like the typed seam"
    );
}

// @internal
#[test]
fn ble_terminal_event_via_dispatch_json_fires_invalidation_like_the_typed_seam() {
    // The typed seam fires a presentation invalidation when the BLE machine
    // reaches a terminal event; without it a flipped Failed screen never
    // reaches the frontend (`2026-06-06-android-ble-execution`). The
    // canonical envelope must fire it identically.
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);

    let calls: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    engine
        .set_event_listener(Box::new(RecordingListener {
            calls: Arc::clone(&calls),
        }))
        .expect("register listener");

    engine
        .navigate_to_json_for_test(r#"{"BleExchange":{"mode":"magic"}}"#.into())
        .expect("navigate to BLE exchange");
    engine
        .dispatch_json(
            r#"{"BleDeviceDiscovered": {"id": "AA:BB:CC:DD:EE:FF", "rssi": -40, "adv_data": [255, 255, 255, 255]}}"#
                .into(),
        )
        .expect("discovery via canonical envelope");

    // A malformed data chunk drives the machine to a terminal Failed event.
    engine
        .dispatch_json(
            r#"{"BleCharacteristicNotified": {"device_id": "peer-1", "direction": "Outbound", "uuid": "a1b2c3d4-e5f6-7890-abcd-ef1234567897", "data": [0, 0, 0, 0, 0, 0, 0, 0]}}"#
                .into(),
        )
        .expect("malformed chunk via canonical envelope");

    assert_eq!(
        current_surface_title(&engine),
        "Failed",
        "terminal machine failure must flip the chrome to the failed screen"
    );
    assert!(
        !calls.lock().unwrap().is_empty(),
        "canonical envelope must fire a presentation invalidation on terminal BLE events"
    );
}

// @internal
#[test]
fn glance_discovery_via_dispatch_json_connects_to_the_scanned_peer_like_the_typed_seam() {
    // F1 dissolves via asymmetric connect: the Glance scanner connects only
    // to the advertiser whose identity matches the scanned QR. The typed
    // seam routes that through `handle_glance_discovery`; the canonical
    // envelope must do the same or the one-sided flow never connects.
    use vauchi_core::exchange::oob_bootstrap::OobBootstrapQr;

    let (a, _da) = create_engine();
    drive_onboarding(&a);
    let (b, _db) = create_engine();
    drive_onboarding(&b);

    for e in [&a, &b] {
        e.set_device_capabilities_json(r#"{"has_camera": true, "has_ble": true}"#.into())
            .expect("report camera + BLE capabilities at boot");
        e.navigate_to_json_for_test(r#""Exchange""#.into())
            .expect("navigate to Exchange");
        select_exchange_mode(e, "Glance");
    }

    let b_qr = qr_data_from_batch(&b);
    let b_identity = *OobBootstrapQr::from_data_string(&b_qr)
        .expect("B's QR parses")
        .identity_key();

    scan_qr_via_capture_binding(&a, &b_qr);

    let foreign = a
        .dispatch_json(
            r#"{"BleDeviceDiscovered": {"id": "mallory-device", "rssi": -40, "adv_data": [171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171]}}"#
                .into(),
        )
        .expect("foreign discovery accepted");
    assert!(
        !foreign.contains("mallory-device"),
        "A must ignore an advertiser it did not scan (F1 dissolves), got: {foreign}",
    );

    let discovery = a
        .dispatch_json(
            serde_json::json!({
                "BleDeviceDiscovered": { "id": "b-device", "rssi": -40, "adv_data": b_identity.to_vec() }
            })
            .to_string(),
        )
        .expect("B discovery accepted");
    assert!(
        discovery.contains("BleConnect") && discovery.contains("b-device"),
        "A must connect specifically to the scanned peer via the canonical envelope, got: {discovery}",
    );
}
