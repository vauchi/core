// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pre-identity onboarding workflow exported to mobile via UniFFI.
//!
//! Every post-identity screen is driven by
//! [`PlatformAppEngine`](crate::PlatformAppEngine), which provides unified
//! navigation, screen rendering, and action handling through a single
//! entry point. `PlatformAppEngine` cannot be constructed before an
//! identity exists (it opens the encrypted DB in `new()`), so onboarding
//! keeps its own workflow type until the primary SMK is derived and
//! storage is available.
//!
//! See: `_private/docs/problems/2026-04-04-core-gui-architecture-alignment/`
//! (Phase 1, Task 1C — the deprecated per-screen `Mobile*Workflow`
//! wrappers for post-identity screens were removed once iOS and Android
//! finished migrating to `PlatformAppEngine`).

use std::sync::Mutex;

use vauchi_app::ui::{OnboardingEngine, WorkflowEngine};

use super::error::MobileError;
use super::json_helpers::{action_result_to_json, screen_to_json, user_action_from_json};

// ── MobileOnboardingWorkflow ────────────────────────────────────────

/// Stateful onboarding workflow for mobile platforms.
///
/// **Deprecated**: Use [`PlatformAppEngine`](crate::PlatformAppEngine) instead.
/// `PlatformAppEngine` wraps the full `AppEngine` orchestrator, providing
/// unified navigation, validation error resolution, and engine caching.
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
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Failed to lock onboarding engine: {e}"),
        })?;
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
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Failed to lock onboarding engine: {e}"),
        })?;
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
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Failed to lock onboarding engine: {e}"),
        })?;
        serde_json::to_string(engine.data()).map_err(|e| MobileError::Other {
            detail: format!("Failed to serialize OnboardingData: {e}"),
        })
    }
}
