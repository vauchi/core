// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
