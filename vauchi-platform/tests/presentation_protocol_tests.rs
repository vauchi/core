// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::MAX_EVENT_JSON_BYTES;
use vauchi_platform::PlatformAppEngine;

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

/// Feature: generic_presentation_protocol.feature
/// Scenario: Binding transports generic commands and events
// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn test_platform_binding_exposes_context_bar_and_routes_primary_activation() {
    let (engine, _dir) = create_engine();
    let commands: serde_json::Value =
        serde_json::from_str(&engine.initial_commands_json().expect("initial commands"))
            .expect("valid commands JSON");
    let set_bar = commands["commands"]
        .as_array()
        .and_then(|commands| {
            commands
                .iter()
                .find_map(|command| command.get("SetContextBar"))
        })
        .expect("context bar command");
    assert_eq!(set_bar["surface_id"], "onboarding");
    let primary_id = set_bar["bar"]["primary"]["interaction_id"]
        .as_str()
        .expect("onboarding primary interaction");

    let event = serde_json::json!({
        "ActionActivated": {
            "surface_id": "onboarding",
            "interaction_id": primary_id,
        }
    });
    let result: serde_json::Value = serde_json::from_str(
        &engine
            .dispatch_json(event.to_string())
            .expect("route primary interaction"),
    )
    .expect("valid event result JSON");
    assert_eq!(
        result["commands"][0]["ReplaceSurface"]["surface"]["revision"],
        2
    );
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn test_platform_binding_refreshes_through_canonical_invalidation_event() {
    let (engine, _dir) = create_engine();
    engine
        .initial_commands_json()
        .expect("prepare initial presentation");

    let result: serde_json::Value = serde_json::from_str(
        &engine
            .dispatch_json(r#""PresentationInvalidated""#.into())
            .expect("dispatch presentation invalidation"),
    )
    .expect("valid command envelope");

    assert_eq!(
        result["commands"][0]["ReplaceSurface"]["surface"]["surface_id"],
        "onboarding"
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario Outline: Available window drives structural composition
// @scenario: generic_presentation_protocol.feature :: Available window drives structural composition
#[test]
fn test_platform_binding_forwards_available_window_facts_to_core() {
    let (engine, _dir) = create_engine();
    let event = serde_json::json!({
        "PresentationEnvironmentChanged": {
            "available_width": 600,
            "available_height": 900,
            "input_modes": ["touch"],
            "motion": "reduced",
        }
    });

    let result: serde_json::Value = serde_json::from_str(
        &engine
            .dispatch_json(event.to_string())
            .expect("handle environment"),
    )
    .expect("valid event result JSON");
    let profile = &result["commands"][0]["SetPresentationProfile"]["profile"];
    assert_eq!(profile["window_class"], "medium");
    assert_eq!(profile["pane_layout"], "single");
    assert_eq!(profile["active_surface"], "onboarding");
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_platform_binding_rejects_oversized_event_json() {
    let (engine, _dir) = create_engine();
    let event = format!(
        "\"PresentationInvalidated\"{padding}",
        padding = " ".repeat(MAX_EVENT_JSON_BYTES)
    );

    let error = engine
        .dispatch_json(event)
        .expect_err("oversized event JSON must be rejected");

    match error {
        vauchi_platform::MobileError::InvalidInput { field, detail } => {
            assert_eq!(field, "");
            assert_eq!(
                detail,
                format!("event JSON exceeds {MAX_EVENT_JSON_BYTES} bytes")
            );
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}
