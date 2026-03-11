// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the mobile UI bindings (MobileOnboardingWorkflow).
//!
//! Verifies that the JSON transport layer correctly serializes/deserializes
//! the core UI types across the FFI boundary.

use vauchi_platform::{
    MobileBackupRecoveryWorkflow, MobileContactEditWorkflow, MobileContactListWorkflow,
    MobileDeliveryStatusWorkflow, MobileDeviceLinkingWorkflow, MobileDuressPinWorkflow,
    MobileEmergencyShredWorkflow, MobileExchangeWorkflow, MobileHelpWorkflow, MobileHomeWorkflow,
    MobileLockScreenWorkflow, MobileOnboardingWorkflow, MobileSettingsWorkflow,
};

// ============================================================================
// MobileOnboardingWorkflow — construction and initial screen
// ============================================================================

#[test]
fn workflow_new_returns_identity_check_screen() {
    let workflow = MobileOnboardingWorkflow::new();
    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse JSON");

    assert_eq!(screen["screen_id"], "identity_check");
    assert_eq!(screen["title"], "Welcome to Vauchi");
    assert!(screen["components"].is_array());
    assert!(screen["actions"].is_array());
    assert!(
        screen["progress"].is_null(),
        "Pre-gate screens have no progress"
    );
}

/// Helper: navigate past IdentityCheck to Welcome.
fn advance_to_welcome(workflow: &MobileOnboardingWorkflow) {
    workflow
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .unwrap();
}

// ============================================================================
// MobileOnboardingWorkflow — action handling round-trip
// ============================================================================

#[test]
fn workflow_handle_action_navigates_to_default_name() {
    let workflow = MobileOnboardingWorkflow::new();
    advance_to_welcome(&workflow);

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
    advance_to_welcome(&workflow);

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
    advance_to_welcome(&workflow);

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
    advance_to_welcome(&workflow);

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
    advance_to_welcome(&workflow);

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

// ============================================================================
// MobileHomeWorkflow
// ============================================================================

#[test]
fn home_workflow_returns_home_screen() {
    let workflow = MobileHomeWorkflow::new(r#"{"completed_steps":3,"total_steps":6}"#.into())
        .expect("should construct");

    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "my_info");
}

#[test]
fn home_workflow_handles_action() {
    let workflow = MobileHomeWorkflow::new(r#"{"completed_steps":6,"total_steps":6}"#.into())
        .expect("should construct");

    let result_json = workflow
        .handle_action_json(r#"{"ActionPressed":{"action_id":"toggle_view"}}"#.into())
        .expect("should handle action");
    let result: serde_json::Value = serde_json::from_str(&result_json).expect("should parse");
    assert!(
        result["UpdateScreen"].is_object(),
        "expected UpdateScreen, got: {result}"
    );
}

// ============================================================================
// MobileContactListWorkflow
// ============================================================================

#[test]
fn contact_list_workflow_returns_screen() {
    let workflow = MobileContactListWorkflow::new(
        r#"[{"id":"c1","name":"Alice","subtitle":null,"avatar_initials":"A","status":null}]"#
            .into(),
    )
    .expect("should construct");

    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "contact_list");
}

// ============================================================================
// MobileSettingsWorkflow
// ============================================================================

#[test]
fn settings_workflow_returns_screen() {
    let config = r#"{
        "display_name": "Alice",
        "delivery_receipts_enabled": true,
        "suppress_presence": false,
        "relay_url": "wss://relay.vauchi.app",
        "device_count": 2,
        "password_set": true
    }"#;
    let workflow = MobileSettingsWorkflow::new(config.into()).expect("should construct");

    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "settings");
}

// ============================================================================
// MobileHelpWorkflow
// ============================================================================

#[test]
fn help_workflow_returns_screen() {
    let items = r#"[{"id":"faq1","question":"How?","answer_url":"https://example.com","category":"General"}]"#;
    let workflow = MobileHelpWorkflow::new(items.into()).expect("should construct");

    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "help");
}

// ============================================================================
// MobileDeliveryStatusWorkflow
// ============================================================================

#[test]
fn delivery_workflow_returns_screen() {
    let items = r#"[{"contact_id":"c1","contact_name":"Alice","status":"Success","detail":null,"retryable":false}]"#;
    let workflow = MobileDeliveryStatusWorkflow::new(items.into()).expect("should construct");

    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "delivery_status");
}

// ============================================================================
// MobileLockScreenWorkflow
// ============================================================================

#[test]
fn lock_screen_workflow_returns_screen() {
    let workflow = MobileLockScreenWorkflow::new(5).expect("should construct");

    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "lock_screen");
}

#[test]
fn lock_screen_workflow_unlock_flow() {
    let workflow = MobileLockScreenWorkflow::new(5).expect("should construct");

    // Enter PIN
    workflow
        .handle_action_json(r#"{"TextChanged":{"component_id":"pin","value":"123456"}}"#.into())
        .expect("should handle text change");

    // Submit
    let result_json = workflow
        .handle_action_json(r#"{"ActionPressed":{"action_id":"unlock"}}"#.into())
        .expect("should handle unlock");

    assert_eq!(result_json.trim_matches('"'), "Complete");
}

// ============================================================================
// MobileContactEditWorkflow
// ============================================================================

#[test]
fn contact_edit_workflow_returns_screen() {
    let contact = r#"{"display_name":"Alice","fields":[{"id":"f1","field_type":"Phone","label":"Mobile","value":"+1-555-0100","visible_to_groups":["Family"],"shown":true}]}"#;
    let groups = r#"["Family","Friends"]"#;
    let workflow =
        MobileContactEditWorkflow::new(contact.into(), groups.into()).expect("should construct");

    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "edit_fields");
}

#[test]
fn contact_edit_workflow_full_flow() {
    let contact = r#"{"display_name":"Alice","fields":[]}"#;
    let groups = r#"["Family"]"#;
    let workflow =
        MobileContactEditWorkflow::new(contact.into(), groups.into()).expect("should construct");

    // EditFields -> EditVisibility
    let result = workflow
        .handle_action_json(r#"{"ActionPressed":{"action_id":"continue"}}"#.into())
        .expect("should navigate");
    let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
    assert!(r["NavigateTo"].is_object(), "expected NavigateTo, got: {r}");

    // EditVisibility -> Preview
    let result = workflow
        .handle_action_json(r#"{"ActionPressed":{"action_id":"continue"}}"#.into())
        .expect("should navigate");
    let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
    assert!(r["NavigateTo"].is_object(), "expected NavigateTo, got: {r}");

    // Save
    let result = workflow
        .handle_action_json(r#"{"ActionPressed":{"action_id":"save"}}"#.into())
        .expect("should complete");
    assert_eq!(result.trim_matches('"'), "Complete");
}

// ============================================================================
// MobileExchangeWorkflow
// ============================================================================

#[test]
fn exchange_workflow_returns_screen() {
    let config = r#"{"own_name":"Alice","own_qr_data":"vauchi://exchange?token=abc"}"#;
    let workflow = MobileExchangeWorkflow::new(config.into()).expect("should construct");
    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "exchange_show_qr");
}

// ============================================================================
// MobileDeviceLinkingWorkflow
// ============================================================================

#[test]
fn device_linking_workflow_returns_screen() {
    let workflow = MobileDeviceLinkingWorkflow::new("vauchi://link?token=abc".into())
        .expect("should construct");
    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "link_show_qr");
}

// ============================================================================
// MobileBackupRecoveryWorkflow
// ============================================================================

#[test]
fn backup_workflow_returns_screen() {
    let workflow = MobileBackupRecoveryWorkflow::new("null".into()).expect("should construct");
    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "backup_choose");
}

#[test]
fn backup_workflow_create_mode() {
    let workflow =
        MobileBackupRecoveryWorkflow::new(r#""Create""#.into()).expect("should construct");
    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "backup_password");
}

// ============================================================================
// MobileDuressPinWorkflow
// ============================================================================

#[test]
fn duress_pin_workflow_returns_screen() {
    let config =
        r#"{"enabled":false,"alert_contacts":[],"alert_message":"Help","include_location":true}"#;
    let workflow = MobileDuressPinWorkflow::new(config.into()).expect("should construct");
    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "duress_overview");
}

// ============================================================================
// MobileEmergencyShredWorkflow
// ============================================================================

#[test]
fn emergency_shred_workflow_returns_screen() {
    let workflow = MobileEmergencyShredWorkflow::new().expect("should construct");
    let json = workflow.current_screen_json().expect("should serialize");
    let screen: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(screen["screen_id"], "shred_warning");
}
