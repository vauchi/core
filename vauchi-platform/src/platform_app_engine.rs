// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unified AppEngine wrapper for mobile/desktop platforms via UniFFI.
//!
//! `PlatformAppEngine` wraps `AppEngine` and exposes the same
//! JSON-based transport format used by `Mobile*Workflow` objects. Unlike those
//! individual workflow wrappers, `PlatformAppEngine` manages navigation, engine
//! lifecycle, and data persistence as a single unified entry point.
//!
//! Mobile apps create this alongside `VauchiPlatform`:
//! - `VauchiPlatform` handles lifecycle, storage, and hardware-bound operations
//! - `PlatformAppEngine` handles navigation, screen rendering, and user actions
//!
//! After mutations via `VauchiPlatform`, call `invalidate_all()` so the next
//! `current_screen_json()` rebuilds engines with fresh data from storage.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vauchi_app::ui::{AppEngine, WorkflowEngine};
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;

use crate::error::MobileError;
use crate::json_helpers::{
    action_result_to_json, app_screen_from_json, screen_to_json, user_action_from_json,
};

// ── PlatformAppEngine ───────────────────────────────────────────────

/// Unified navigation and screen engine for mobile/desktop platforms.
///
/// Wraps `AppEngine` with JSON-based FFI transport.
/// Manages screen navigation, engine lifecycle, and form dialog persistence.
///
/// # Usage from Swift/Kotlin
///
/// ```swift
/// let engine = try PlatformAppEngine(
///     dataDir: dataDir,
///     relayUrl: "wss://relay.vauchi.app",
///     storageKeyBytes: keyBytes
/// )
///
/// // Get current screen
/// let screenJson = try engine.currentScreenJson()
///
/// // Handle user action
/// let resultJson = try engine.handleActionJson(
///     actionJson: "{\"ActionPressed\": {\"action_id\": \"get_started\"}}"
/// )
///
/// // Navigate to a screen
/// let screenJson = try engine.navigateToJson(screenJson: "\"Exchange\"")
///
/// // After VauchiPlatform mutations, invalidate
/// try engine.invalidateAll()
/// ```
#[derive(uniffi::Object)]
pub struct PlatformAppEngine {
    engine: Mutex<AppEngine>,
}

#[uniffi::export]
impl PlatformAppEngine {
    /// Create a new PlatformAppEngine with platform-provided secure key.
    ///
    /// This creates its own `Vauchi` instance backed by the same database
    /// as `VauchiPlatform`. After mutations via `VauchiPlatform`, call
    /// `invalidate_all()` to refresh cached engines.
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        relay_url: String,
        storage_key_bytes: Vec<u8>,
    ) -> Result<Arc<Self>, MobileError> {
        let data_path = PathBuf::from(&data_dir);

        std::fs::create_dir_all(&data_path)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let storage_path = data_path.join("vauchi.db");

        let key_array: [u8; 32] = storage_key_bytes.try_into().map_err(|_| {
            MobileError::StorageError("Storage key must be exactly 32 bytes".to_string())
        })?;
        let storage_key = SymmetricKey::from_bytes_unchecked(key_array);

        let config = VauchiConfig::with_storage_path(&storage_path)
            .with_relay_url(&relay_url)
            .with_storage_key(storage_key);

        let vauchi = Vauchi::new(config).map_err(|e| MobileError::Internal(e.to_string()))?;

        Ok(Arc::new(Self {
            engine: Mutex::new(AppEngine::new(vauchi)),
        }))
    }

    /// Returns the current screen as a JSON string.
    ///
    /// The JSON structure matches `ScreenModel` from vauchi-core.
    pub fn current_screen_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        screen_to_json(&engine.current_screen())
    }

    /// Handles a user action (as JSON) and returns the result as JSON.
    ///
    /// The action JSON must match the `UserAction` enum format.
    /// The result JSON matches the `ActionResult` enum.
    pub fn handle_action_json(&self, action_json: String) -> Result<String, MobileError> {
        let action = user_action_from_json(&action_json)?;
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let result = engine.handle_action(action);
        action_result_to_json(&result)
    }

    /// Navigate to a screen (as JSON) and return the new screen model as JSON.
    ///
    /// The screen JSON must match the `AppScreen` enum format, e.g.:
    /// - `"Exchange"` (simple variant)
    /// - `{"ContactDetail": {"contact_id": "abc"}}` (parameterized variant)
    pub fn navigate_to_json(&self, screen_json: String) -> Result<String, MobileError> {
        let screen = app_screen_from_json(&screen_json)?;
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let model = engine.navigate_to(screen);
        screen_to_json(&model)
    }

    /// Navigate back in the history stack.
    ///
    /// Returns the previous screen model as JSON.
    pub fn navigate_back_json(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let model = engine.navigate_back();
        screen_to_json(&model)
    }

    /// Returns the available navigation screens as a JSON array.
    ///
    /// These are the screens that should appear in the navigation bar/tabs.
    pub fn available_screens_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let screens = engine.available_screens();
        serde_json::to_string(&screens)
            .map_err(|e| MobileError::Internal(format!("Failed to serialize screens: {e}")))
    }

    /// Returns the default landing screen as a JSON string.
    ///
    /// Returns MyInfo when no contacts, Contacts when >=1 contact.
    pub fn default_screen_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let screen = engine.default_screen();
        serde_json::to_string(&screen)
            .map_err(|e| MobileError::Internal(format!("Failed to serialize screen: {e}")))
    }

    /// Returns the current screen's screen_id (lightweight query).
    ///
    /// Useful for tab bar highlighting without deserializing the full ScreenModel.
    pub fn current_screen_id(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let model = engine.current_screen();
        Ok(model.screen_id)
    }

    /// Invalidate all cached engines.
    ///
    /// Call this after mutations via `VauchiPlatform` so the next
    /// `current_screen_json()` rebuilds engines with fresh data.
    pub fn invalidate_all(&self) -> Result<(), MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.invalidate_all();
        Ok(())
    }

    /// Invalidate a specific screen's cached engine.
    ///
    /// The screen JSON must match the `AppScreen` enum format.
    pub fn invalidate_screen_json(&self, screen_json: String) -> Result<(), MobileError> {
        let screen = app_screen_from_json(&screen_json)?;
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.invalidate_screen(&screen);
        Ok(())
    }

    /// Returns whether the user has created an identity.
    ///
    /// Used by frontends to decide between onboarding and main UI.
    pub fn has_identity(&self) -> Result<bool, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        Ok(engine.has_identity())
    }

    /// Returns whether the current form has unsaved user data.
    ///
    /// Used by frontends to show a "discard changes?" prompt on back navigation.
    pub fn form_has_data(&self) -> Result<bool, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        Ok(engine.form_has_data())
    }
}
