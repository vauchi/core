// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UI type exports for mobile platforms via UniFFI.
//!
//! **DEPRECATED**: All `Mobile*Workflow` types in this module are superseded by
//! [`PlatformAppEngine`](crate::PlatformAppEngine), which provides unified
//! navigation, screen rendering, and action handling through a single entry
//! point. New code should use `PlatformAppEngine` exclusively. These per-screen
//! workflow types will be removed once iOS and Android have migrated.
//!
//! See: `_private/docs/problems/2026-04-04-core-gui-architecture-alignment/`
//!
//! The core UI types (`ScreenModel`, `Component`, `UserAction`, `ActionResult`)
//! are deeply nested enums with struct variants. Rather than creating dozens of
//! Mobile* wrapper types (which would be fragile and hard to maintain), we use
//! JSON serialization as the transport format:
//!
//! - Core types already derive `Serialize`/`Deserialize`
//! - Swift uses `Codable`, Kotlin uses `kotlinx.serialization`
//! - Adding new Component variants doesn't require FFI changes

use std::sync::Mutex;

use vauchi_app::ui::{
    BackupMode, BackupRecoveryEngine, ContactEditEngine, ContactItem, ContactListEngine,
    DeliveryItem, DeliveryStatusEngine, DeviceLinkingEngine, DuressConfig, DuressPinEngine,
    EditableContact, EmergencyShredEngine, ExchangeConfig, ExchangeEngine, HelpEngine, HelpItem,
    LockScreenEngine, MyInfoEngine, MyInfoProgress, OnboardingEngine, SettingsConfig,
    SettingsEngine, WorkflowEngine,
};

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

// ── Helper macro for boilerplate ──────────────────────────────────

/// Generates a Mobile*Workflow struct with `current_screen_json` and `handle_action_json`.
///
/// **Deprecated**: All types generated by this macro are superseded by
/// [`PlatformAppEngine`](crate::PlatformAppEngine). Use `PlatformAppEngine`
/// for all new code.
macro_rules! mobile_workflow {
    (
        $name:ident wraps $engine:ty {
            constructor($($param:ident : $ptype:ty),*) -> $parse:expr_2021
        }
    ) => {
        #[derive(uniffi::Object)]
        pub struct $name {
            engine: Mutex<$engine>,
        }

        #[uniffi::export]
        impl $name {
            #[uniffi::constructor]
            pub fn new($($param: $ptype),*) -> Result<Self, MobileError> {
                let engine = $parse;
                Ok(Self {
                    engine: Mutex::new(engine),
                })
            }

            pub fn current_screen_json(&self) -> Result<String, MobileError> {
                let engine = self
                    .engine
                    .lock()
                    .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
                screen_to_json(&engine.current_screen())
            }

            pub fn handle_action_json(&self, action_json: String) -> Result<String, MobileError> {
                let action = user_action_from_json(&action_json)?;
                let mut engine = self
                    .engine
                    .lock()
                    .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
                action_result_to_json(&engine.handle_action(action))
            }
        }
    };
}

// ── MobileHomeWorkflow ────────────────────────────────────────────

mobile_workflow! {
    MobileHomeWorkflow wraps MyInfoEngine {
        constructor(progress_json: String) -> {
            let progress: MyInfoProgress = serde_json::from_str(&progress_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse progress: {e}")))?;
            MyInfoEngine::new(progress)
        }
    }
}

// ── MobileContactListWorkflow ─────────────────────────────────────

mobile_workflow! {
    MobileContactListWorkflow wraps ContactListEngine {
        constructor(contacts_json: String) -> {
            let contacts: Vec<ContactItem> = serde_json::from_str(&contacts_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse contacts: {e}")))?;
            ContactListEngine::new(contacts)
        }
    }
}

// ── MobileSettingsWorkflow ────────────────────────────────────────

mobile_workflow! {
    MobileSettingsWorkflow wraps SettingsEngine {
        constructor(config_json: String) -> {
            let config: SettingsConfig = serde_json::from_str(&config_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse config: {e}")))?;
            SettingsEngine::new(config)
        }
    }
}

// ── MobileHelpWorkflow ────────────────────────────────────────────

mobile_workflow! {
    MobileHelpWorkflow wraps HelpEngine {
        constructor(items_json: String) -> {
            let items: Vec<HelpItem> = serde_json::from_str(&items_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse help items: {e}")))?;
            HelpEngine::new(items)
        }
    }
}

// ── MobileDeliveryStatusWorkflow ──────────────────────────────────

mobile_workflow! {
    MobileDeliveryStatusWorkflow wraps DeliveryStatusEngine {
        constructor(items_json: String) -> {
            let items: Vec<DeliveryItem> = serde_json::from_str(&items_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse delivery items: {e}")))?;
            DeliveryStatusEngine::new(items)
        }
    }
}

// ── MobileLockScreenWorkflow ──────────────────────────────────────

mobile_workflow! {
    MobileLockScreenWorkflow wraps LockScreenEngine {
        constructor(max_attempts: u32) -> {
            LockScreenEngine::new(max_attempts as usize)
        }
    }
}

// ── MobileContactEditWorkflow ─────────────────────────────────────

mobile_workflow! {
    MobileContactEditWorkflow wraps ContactEditEngine {
        constructor(contact_json: String, groups_json: String) -> {
            let contact: EditableContact = serde_json::from_str(&contact_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse contact: {e}")))?;
            let groups: Vec<String> = serde_json::from_str(&groups_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse groups: {e}")))?;
            ContactEditEngine::new(contact, groups)
        }
    }
}

// ── MobileExchangeWorkflow ──────────────────────────────────────

mobile_workflow! {
    MobileExchangeWorkflow wraps ExchangeEngine {
        constructor(config_json: String) -> {
            let config: ExchangeConfig = serde_json::from_str(&config_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse exchange config: {e}")))?;
            ExchangeEngine::new(config)
        }
    }
}

#[uniffi::export]
impl MobileExchangeWorkflow {
    /// Signal that exchange verification succeeded.
    pub fn mark_success(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.mark_success();
        screen_to_json(&engine.current_screen())
    }

    /// Signal that exchange verification failed.
    pub fn mark_failed(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.mark_failed();
        screen_to_json(&engine.current_screen())
    }

    /// Returns the scanned QR data, if any.
    pub fn scanned_data(&self) -> Result<Option<String>, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        Ok(engine.scanned_data().map(|s| s.to_string()))
    }
}

// ── MobileDeviceLinkingWorkflow ─────────────────────────────────

mobile_workflow! {
    MobileDeviceLinkingWorkflow wraps DeviceLinkingEngine {
        constructor(qr_data: String) -> {
            DeviceLinkingEngine::new(qr_data)
        }
    }
}

#[uniffi::export]
impl MobileDeviceLinkingWorkflow {
    /// Signal that a peer device has connected with a verification code.
    pub fn peer_connected(&self, verification_code: String) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.peer_connected(verification_code);
        screen_to_json(&engine.current_screen())
    }

    /// Signal that data sync has completed.
    pub fn sync_complete(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.sync_complete();
        screen_to_json(&engine.current_screen())
    }
}

// ── MobileBackupRecoveryWorkflow ────────────────────────────────

mobile_workflow! {
    MobileBackupRecoveryWorkflow wraps BackupRecoveryEngine {
        constructor(mode_json: String) -> {
            let mode: Option<BackupMode> = serde_json::from_str(&mode_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse backup mode: {e}")))?;
            BackupRecoveryEngine::new(mode)
        }
    }
}

#[uniffi::export]
impl MobileBackupRecoveryWorkflow {
    /// Signal that async processing completed successfully.
    pub fn processing_complete(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.processing_complete();
        screen_to_json(&engine.current_screen())
    }

    /// Signal that async processing failed.
    pub fn processing_failed(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.processing_failed();
        screen_to_json(&engine.current_screen())
    }
}

// ── MobileDuressPinWorkflow ─────────────────────────────────────

mobile_workflow! {
    MobileDuressPinWorkflow wraps DuressPinEngine {
        constructor(config_json: String) -> {
            let config: DuressConfig = serde_json::from_str(&config_json)
                .map_err(|e| MobileError::InvalidInput(format!("Failed to parse duress config: {e}")))?;
            DuressPinEngine::new(config)
        }
    }
}

#[uniffi::export]
impl MobileDuressPinWorkflow {
    /// Returns the current duress config as JSON.
    pub fn config_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        serde_json::to_string(engine.config())
            .map_err(|e| MobileError::Internal(format!("Failed to serialize DuressConfig: {e}")))
    }
}

// ── MobileEmergencyShredWorkflow ────────────────────────────────

mobile_workflow! {
    MobileEmergencyShredWorkflow wraps EmergencyShredEngine {
        constructor() -> {
            EmergencyShredEngine::new()
        }
    }
}

#[uniffi::export]
impl MobileEmergencyShredWorkflow {
    /// Signal that the wipe operation has finished.
    pub fn wipe_complete(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.wipe_complete();
        screen_to_json(&engine.current_screen())
    }
}

// ── MobileLockScreenWorkflow extra methods ──────────────────────

#[uniffi::export]
impl MobileLockScreenWorkflow {
    /// Record a failed unlock attempt. Returns the updated screen JSON.
    /// Check the response for lockout state.
    pub fn record_failed_attempt(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let _locked_out = engine.record_failed_attempt();
        screen_to_json(&engine.current_screen())
    }
}
