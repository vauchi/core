// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UI type exports for mobile platforms via UniFFI.
//!
//! The core UI types (`ScreenModel`, `Component`, `UserAction`, `ActionResult`)
//! are deeply nested enums with struct variants. Rather than creating dozens of
//! Mobile* wrapper types (which would be fragile and hard to maintain), we use
//! JSON serialization as the transport format:
//!
//! - Core types already derive `Serialize`/`Deserialize`
//! - Swift uses `Codable`, Kotlin uses `kotlinx.serialization`
//! - Adding new Component variants doesn't require FFI changes
//!
//! The `MobileOnboardingWorkflow` object wraps `OnboardingEngine` and exposes
//! `current_screen_json()` / `handle_action_json()` for stateful interaction.

use std::sync::Mutex;

use vauchi_core::ui::{ActionResult, OnboardingEngine, ScreenModel, UserAction, WorkflowEngine};

use super::error::MobileError;

// ── JSON transport helpers ──────────────────────────────────────────

/// Serialize a ScreenModel to JSON.
fn screen_to_json(screen: &ScreenModel) -> Result<String, MobileError> {
    serde_json::to_string(screen)
        .map_err(|e| MobileError::Internal(format!("Failed to serialize ScreenModel: {e}")))
}

/// Serialize an ActionResult to JSON.
fn action_result_to_json(result: &ActionResult) -> Result<String, MobileError> {
    serde_json::to_string(result)
        .map_err(|e| MobileError::Internal(format!("Failed to serialize ActionResult: {e}")))
}

/// Deserialize a UserAction from JSON.
fn user_action_from_json(json: &str) -> Result<UserAction, MobileError> {
    serde_json::from_str(json)
        .map_err(|e| MobileError::InvalidInput(format!("Failed to parse UserAction JSON: {e}")))
}

// ── MobileOnboardingWorkflow ────────────────────────────────────────

/// Stateful onboarding workflow for mobile platforms.
///
/// Wraps the core `OnboardingEngine` state machine. All screen data and
/// action results are exchanged as JSON strings — see module docs for why.
///
/// # Usage from Swift/Kotlin
///
/// ```swift
/// let workflow = MobileOnboardingWorkflow()
/// let screenJson = try workflow.currentScreenJson()
/// // decode screenJson into your ScreenModel Codable struct
///
/// let actionJson = """
///   {"ActionPressed": {"action_id": "get_started"}}
/// """
/// let resultJson = try workflow.handleActionJson(actionJson: actionJson)
/// ```
#[derive(uniffi::Object)]
pub struct MobileOnboardingWorkflow {
    engine: Mutex<OnboardingEngine>,
}

impl Default for MobileOnboardingWorkflow {
    fn default() -> Self {
        Self::new()
    }
}

#[uniffi::export]
impl MobileOnboardingWorkflow {
    /// Creates a new onboarding workflow starting at the Welcome screen.
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(OnboardingEngine::new()),
        }
    }

    /// Returns the current screen as a JSON string.
    ///
    /// The JSON structure matches `ScreenModel` from vauchi-core:
    /// ```json
    /// {
    ///   "screen_id": "welcome",
    ///   "title": "Welcome to Vauchi",
    ///   "subtitle": "Your contacts, your rules.",
    ///   "components": [...],
    ///   "actions": [...],
    ///   "progress": { "current_step": 1, "total_steps": 9, "label": null }
    /// }
    /// ```
    pub fn current_screen_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Failed to lock onboarding engine: {e}")))?;
        screen_to_json(&engine.current_screen())
    }

    /// Handles a user action (as JSON) and returns the result as JSON.
    ///
    /// The action JSON must match the `UserAction` enum format:
    /// - `{"ActionPressed": {"action_id": "get_started"}}`
    /// - `{"TextChanged": {"component_id": "display_name", "value": "Alice"}}`
    /// - `{"ItemToggled": {"component_id": "groups", "item_id": "Family"}}`
    /// - `{"FieldVisibilityChanged": {"field_id": "field_0", "group_id": null, "visible": true}}`
    /// - `{"GroupViewSelected": {"group_name": "Family"}}`
    ///
    /// The result JSON matches the `ActionResult` enum:
    /// - `{"UpdateScreen": {...}}` — re-render the current screen
    /// - `{"NavigateTo": {...}}` — navigate to a new screen
    /// - `{"ValidationError": {"component_id": "...", "message": "..."}}` — show error
    /// - `"Complete"` — onboarding is finished, persist data
    pub fn handle_action_json(&self, action_json: String) -> Result<String, MobileError> {
        let action = user_action_from_json(&action_json)?;
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Failed to lock onboarding engine: {e}")))?;
        let result = engine.handle_action(action);
        action_result_to_json(&result)
    }

    /// Returns the collected onboarding data as JSON when the workflow is complete.
    ///
    /// The JSON structure matches `OnboardingData`:
    /// ```json
    /// {
    ///   "display_name": "Alice",
    ///   "selected_groups": [{"name": "Family", "selected": true, "name_override": null}],
    ///   "fields": [{"field_type": "Phone", "label": "Mobile", "value": "+1...", ...}]
    /// }
    /// ```
    pub fn onboarding_data_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Failed to lock onboarding engine: {e}")))?;
        serde_json::to_string(engine.data())
            .map_err(|e| MobileError::Internal(format!("Failed to serialize OnboardingData: {e}")))
    }
}
