// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the mobile UI bindings (MobileOnboardingWorkflow).
//!
//! Verifies that the JSON transport layer correctly serializes/deserializes
//! the core UI types across the FFI boundary.

use vauchi_mobile::MobileOnboardingWorkflow;

// ============================================================================
// MobileOnboardingWorkflow — construction and initial screen
// ============================================================================

#[test]
fn workflow_new_returns_welcome_screen() {
    let workflow = MobileOnboardingWorkflow::new();
    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse JSON");

    assert_eq!(screen["screen_id"], "welcome");
    assert_eq!(screen["title"], "Welcome to Vauchi");
    assert!(screen["components"].is_array());
    assert!(screen["actions"].is_array());
    assert_eq!(screen["progress"]["current_step"], 1);
    assert_eq!(screen["progress"]["total_steps"], 9);
}

// ============================================================================
// MobileOnboardingWorkflow — action handling round-trip
// ============================================================================

#[test]
fn workflow_handle_action_navigates_to_default_name() {
    let workflow = MobileOnboardingWorkflow::new();

    let action_json = r#"{"ActionPressed": {"action_id": "get_started"}}"#;
    let result_json = workflow
        .handle_action_json(action_json.to_string())
        .expect("should handle action");
    let result: serde_json::Value = serde_json::from_str(&result_json).expect("should parse JSON");

    // Should be NavigateTo with default_name screen
    assert!(
        result["NavigateTo"].is_object(),
        "expected NavigateTo, got: {result}"
    );
    assert_eq!(result["NavigateTo"]["screen_id"], "default_name");
}

#[test]
fn workflow_text_changed_updates_screen() {
    let workflow = MobileOnboardingWorkflow::new();

    // Navigate to default_name
    let nav = r#"{"ActionPressed": {"action_id": "get_started"}}"#;
    workflow.handle_action_json(nav.to_string()).unwrap();

    // Type a name
    let text_action = r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#;
    let result_json = workflow
        .handle_action_json(text_action.to_string())
        .expect("should handle text change");
    let result: serde_json::Value = serde_json::from_str(&result_json).expect("should parse JSON");

    assert!(result["UpdateScreen"].is_object(), "expected UpdateScreen");
    assert_eq!(result["UpdateScreen"]["screen_id"], "default_name");
}

#[test]
fn workflow_validation_error_on_empty_name() {
    let workflow = MobileOnboardingWorkflow::new();

    // Navigate to default_name
    let nav = r#"{"ActionPressed": {"action_id": "get_started"}}"#;
    workflow.handle_action_json(nav.to_string()).unwrap();

    // Try to continue without entering a name
    let continue_action = r#"{"ActionPressed": {"action_id": "continue"}}"#;
    let result_json = workflow
        .handle_action_json(continue_action.to_string())
        .expect("should handle action");
    let result: serde_json::Value = serde_json::from_str(&result_json).expect("should parse JSON");

    assert!(
        result["ValidationError"].is_object(),
        "expected ValidationError, got: {result}"
    );
    assert_eq!(result["ValidationError"]["component_id"], "display_name");
}

// ============================================================================
// MobileOnboardingWorkflow — full flow to completion
// ============================================================================

#[test]
fn workflow_skip_flow_reaches_complete() {
    let workflow = MobileOnboardingWorkflow::new();

    // Welcome -> DefaultName
    workflow
        .handle_action_json(r#"{"ActionPressed": {"action_id": "get_started"}}"#.into())
        .unwrap();

    // Enter name
    workflow
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .unwrap();

    // DefaultName -> SkipGate
    workflow
        .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
        .unwrap();

    // SkipGate -> SecurityExplanation (skip)
    workflow
        .handle_action_json(r#"{"ActionPressed": {"action_id": "skip_to_finish"}}"#.into())
        .unwrap();

    // SecurityExplanation -> BackupPrompt
    workflow
        .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
        .unwrap();

    // BackupPrompt -> Ready (skip)
    workflow
        .handle_action_json(r#"{"ActionPressed": {"action_id": "skip"}}"#.into())
        .unwrap();

    // Ready -> Complete
    let result_json = workflow
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start"}}"#.into())
        .unwrap();

    assert_eq!(result_json.trim_matches('"'), "Complete");
}

// ============================================================================
// MobileOnboardingWorkflow — onboarding data
// ============================================================================

#[test]
fn workflow_onboarding_data_json_returns_valid_data() {
    let workflow = MobileOnboardingWorkflow::new();

    // Navigate and set a name
    workflow
        .handle_action_json(r#"{"ActionPressed": {"action_id": "get_started"}}"#.into())
        .unwrap();
    workflow
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Bob"}}"#.into(),
        )
        .unwrap();

    let data_json = workflow
        .onboarding_data_json()
        .expect("should serialize data");
    let data: serde_json::Value = serde_json::from_str(&data_json).expect("should parse JSON");

    assert_eq!(data["display_name"], "Bob");
    assert!(data["selected_groups"].is_array());
    assert!(data["fields"].is_array());
}

// ============================================================================
// MobileOnboardingWorkflow — invalid input handling
// ============================================================================

#[test]
fn workflow_rejects_invalid_json() {
    let workflow = MobileOnboardingWorkflow::new();

    let result = workflow.handle_action_json("not valid json".into());
    assert!(result.is_err(), "should reject invalid JSON");

    let err = result.unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("Failed to parse UserAction JSON"),
        "error should mention parsing: {err_msg}"
    );
}

#[test]
fn workflow_rejects_unknown_action_variant() {
    let workflow = MobileOnboardingWorkflow::new();

    let result = workflow.handle_action_json(r#"{"UnknownAction": {}}"#.into());
    assert!(result.is_err(), "should reject unknown action variant");
}
